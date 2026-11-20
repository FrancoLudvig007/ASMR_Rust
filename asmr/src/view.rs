use crate::controller::{export_wav_session, start_audio_stream};
use crate::model::{load_f32, store_f32, AudioModel, SCOPE_SIZE};
use crate::pyrsutils::prelude::*;
use eframe::egui::{self, Color32, Pos2, Rect, RichText, Stroke, Vec2};
use std::f32::consts::TAU;
use std::sync::atomic::Ordering;
use std::sync::Arc;

pub struct DeltaAsmrView {
    model: Arc<AudioModel>,
    _stream: Option<cpal::Stream>,
    status_msg: String,
    scope_view_mode: u8, // 0 = Estéreo, 1 = Mix, 2 = Lissajous, 3 = Espectro
    scope_zoom: f32,
    sleep_timer_active: bool,
    sleep_remaining_secs: f64,
    last_frame_time: f64,
}

impl DeltaAsmrView {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let model = Arc::new(AudioModel::new());
        let (stream, status_msg) = match start_audio_stream(Arc::clone(&model)) {
            Ok(s) => (
                Some(s),
                "Motor DSP MVC Polimórfico Ativo (Lock-Free)".to_string(),
            ),
            Err(e) => (None, format!("Modo Visual (Sem Áudio: {e})")),
        };

        print("Delta Waves, Lo-Fi & ASMR Synth (Arquitetura MVC) iniciado!");

        Self {
            model,
            _stream: stream,
            status_msg,
            scope_view_mode: 0,
            scope_zoom: 1.5,
            sleep_timer_active: false,
            sleep_remaining_secs: 1800.0,
            last_frame_time: perf_counter(),
        }
    }

    fn draw_analyzer(&mut self, ui: &mut egui::Ui) {
        ui.group(|ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Analisador Acústico em Tempo Real").strong());
                ui.separator();
                ui.selectable_value(&mut self.scope_view_mode, 0, "Estéreo (L/R)");
                ui.selectable_value(&mut self.scope_view_mode, 1, "Onda Somada (Mix)");
                ui.selectable_value(&mut self.scope_view_mode, 2, "Fase Binaural (Lissajous)");
                ui.selectable_value(&mut self.scope_view_mode, 3, "Espectro (Bandas)");
                ui.separator();
                ui.label("Ganho Visual:");
                ui.add(egui::Slider::new(&mut self.scope_zoom, 0.5..=4.0).text("x"));
            });

            let desired_size = Vec2::new(ui.available_width(), 165.0);
            let (full_rect, _response) = ui.allocate_exact_size(desired_size, egui::Sense::hover());
            let painter = ui.painter_at(full_rect);

            let vu_width = 36.0;
            let rect = Rect::from_min_max(
                full_rect.min,
                Pos2::new(full_rect.max.x - vu_width - 6.0, full_rect.max.y),
            );
            let vu_rect = Rect::from_min_max(
                Pos2::new(full_rect.max.x - vu_width, full_rect.min.y),
                full_rect.max,
            );

            painter.rect_filled(rect, 6.0, Color32::from_rgb(12, 16, 22));
            painter.rect_filled(vu_rect, 4.0, Color32::from_rgb(16, 20, 28));

            let grid_stroke = Stroke::new(1.0, Color32::from_rgb(30, 42, 56));
            for step in range_from(1, 4) {
                let y = rect.top() + rect.height() * (step as f32 / 4.0);
                painter.line_segment(
                    [Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)],
                    grid_stroke,
                );
            }
            for step in range_from(1, 8) {
                let x = rect.left() + rect.width() * (step as f32 / 8.0);
                painter.line_segment(
                    [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
                    grid_stroke,
                );
            }

            let write_head = self.model.scope_write_idx.load(Ordering::Relaxed);
            let mut samples_l = Vec::with_capacity(SCOPE_SIZE);
            let mut samples_r = Vec::with_capacity(SCOPE_SIZE);

            for offset in range(SCOPE_SIZE as i64) {
                let idx = (write_head + offset as usize) % SCOPE_SIZE;
                samples_l.append_item(load_f32(&self.model.scope_l[idx]));
                samples_r.append_item(load_f32(&self.model.scope_r[idx]));
            }

            let rms_l = (sum(samples_l.iter().map(|v| v * v)) / (SCOPE_SIZE as f32)).sqrt();
            let rms_r = (sum(samples_r.iter().map(|v| v * v)) / (SCOPE_SIZE as f32)).sqrt();
            let peak =
                max(samples_l.iter().chain(samples_r.iter()).map(|v| abs(*v))).unwrap_or(0.0);
            let min_val = min(samples_l.iter().copied()).unwrap_or(0.0);

            // Medidor VU Estéreo (L/R)
            let h_l = (rms_l * self.scope_zoom * 1.4).clamp(0.0, 1.0) * (vu_rect.height() - 8.0);
            let h_r = (rms_r * self.scope_zoom * 1.4).clamp(0.0, 1.0) * (vu_rect.height() - 8.0);
            let bar_l = Rect::from_min_max(
                Pos2::new(vu_rect.left() + 4.0, vu_rect.bottom() - 4.0 - h_l),
                Pos2::new(vu_rect.center().x - 1.0, vu_rect.bottom() - 4.0),
            );
            let bar_r = Rect::from_min_max(
                Pos2::new(vu_rect.center().x + 1.0, vu_rect.bottom() - 4.0 - h_r),
                Pos2::new(vu_rect.right() - 4.0, vu_rect.bottom() - 4.0),
            );
            painter.rect_filled(bar_l, 2.0, Color32::from_rgb(70, 210, 255));
            painter.rect_filled(bar_r, 2.0, Color32::from_rgb(255, 165, 70));

            let mid_y = rect.center().y;
            let half_h = rect.height() * 0.45 * self.scope_zoom;

            if self.scope_view_mode == 3 {
                let num_bands = 20usize;
                let window_len = 256usize;
                let bar_w = (rect.width() - 20.0) / (num_bands as f32);

                for b in range(num_bands as i64) {
                    let k = (b as f32) * 1.6 + 1.0;
                    let mut re = 0.0f32;
                    let mut im = 0.0f32;
                    for n in range(window_len as i64) {
                        let idx = SCOPE_SIZE - window_len + (n as usize);
                        let sig = 0.5 * (samples_l[idx] + samples_r[idx]);
                        let angle = TAU * k * (n as f32) / (window_len as f32);
                        re += sig * angle.cos();
                        im -= sig * angle.sin();
                    }
                    let mag = ((re * re + im * im).sqrt() / (window_len as f32)).sqrt();
                    let norm_h =
                        (mag * self.scope_zoom * 2.6).clamp(0.02, 0.95) * (rect.height() - 16.0);

                    let x0 = rect.left() + 10.0 + (b as f32) * bar_w;
                    let bar_rect = Rect::from_min_max(
                        Pos2::new(x0 + 2.0, rect.bottom() - 6.0 - norm_h),
                        Pos2::new(x0 + bar_w - 2.0, rect.bottom() - 6.0),
                    );
                    let color = Color32::from_rgb((60 + b * 8) as u8, (210 - b * 5) as u8, 245);
                    painter.rect_filled(bar_rect, 3.0, color);
                }
            } else if self.scope_view_mode == 2 {
                let center = rect.center();
                let radius = rect.height().min(rect.width()) * 0.44 * self.scope_zoom;
                let pts: Vec<Pos2> = zip(&samples_l, &samples_r)
                    .map(|(&l, &r)| Pos2::new(center.x + l * radius, center.y - r * radius))
                    .collect();
                painter.add(egui::Shape::line(
                    pts,
                    Stroke::new(1.6, Color32::from_rgb(80, 230, 190)),
                ));
            } else if self.scope_view_mode == 1 {
                let pts: Vec<Pos2> = enumerate(zip(&samples_l, &samples_r))
                    .map(|(i, (&l, &r))| {
                        let x = rect.left() + (i as f32 / (SCOPE_SIZE - 1) as f32) * rect.width();
                        let y = (mid_y - 0.5 * (l + r) * half_h).clamp(rect.top(), rect.bottom());
                        Pos2::new(x, y)
                    })
                    .collect();
                painter.add(egui::Shape::line(
                    pts,
                    Stroke::new(1.8, Color32::from_rgb(100, 220, 255)),
                ));
            } else {
                let pts_l: Vec<Pos2> = enumerate(&samples_l)
                    .map(|(i, &l)| {
                        let x = rect.left() + (i as f32 / (SCOPE_SIZE - 1) as f32) * rect.width();
                        let y = (mid_y - l * half_h).clamp(rect.top(), rect.bottom());
                        Pos2::new(x, y)
                    })
                    .collect();

                let pts_r: Vec<Pos2> = enumerate(&samples_r)
                    .map(|(i, &r)| {
                        let x = rect.left() + (i as f32 / (SCOPE_SIZE - 1) as f32) * rect.width();
                        let y = (mid_y - r * half_h).clamp(rect.top(), rect.bottom());
                        Pos2::new(x, y)
                    })
                    .collect();

                painter.add(egui::Shape::line(
                    pts_r,
                    Stroke::new(1.4, Color32::from_rgba_unmultiplied(255, 165, 70, 190)),
                ));
                painter.add(egui::Shape::line(
                    pts_l,
                    Stroke::new(1.6, Color32::from_rgb(70, 210, 255)),
                ));
            }

            let info_text = format!(
                "RMS L/R: {:.3} / {:.3} | Pico: {:.3} | Vale: {:.3}",
                rms_l, rms_r, peak, min_val
            );
            painter.text(
                Pos2::new(rect.left() + 10.0, rect.top() + 8.0),
                egui::Align2::LEFT_TOP,
                info_text,
                egui::FontId::monospace(11.5),
                Color32::from_rgb(160, 190, 215),
            );
        });
    }
}

impl eframe::App for DeltaAsmrView {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let now = perf_counter();
        let dt = (now - self.last_frame_time).max(0.0);
        self.last_frame_time = now;

        if self.sleep_timer_active {
            self.sleep_remaining_secs = (self.sleep_remaining_secs - dt).max(0.0);
            if self.sleep_remaining_secs <= 60.0 && self.sleep_remaining_secs > 0.0 {
                let ratio = (self.sleep_remaining_secs / 60.0) as f32;
                let current_master = load_f32(&self.model.master_vol);
                store_f32(&self.model.master_vol, current_master.min(ratio * ratio));
            } else if self.sleep_remaining_secs <= 0.0 {
                self.model.playing.store(false, Ordering::Relaxed);
                self.sleep_timer_active = false;
            }
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            // Barra Superior de Controle e Presets
            ui.horizontal_wrapped(|ui| {
                let is_playing = self.model.playing.load(Ordering::Relaxed);
                let btn_label = if is_playing {
                    "⏸ Pausar"
                } else {
                    "▶ Tocar"
                };
                if ui
                    .button(RichText::new(btn_label).size(15.0).strong())
                    .clicked()
                {
                    self.model.playing.store(!is_playing, Ordering::Relaxed);
                }

                ui.separator();
                ui.label("Master:");
                let mut master = load_f32(&self.model.master_vol);
                if ui.add(egui::Slider::new(&mut master, 0.0..=1.0)).changed() {
                    store_f32(&self.model.master_vol, master);
                }

                ui.separator();
                if ui.button("Sono Profundo").clicked() {
                    self.model.apply_factory_preset(0);
                }
                if ui.button("Schumann + Riacho").clicked() {
                    self.model.apply_factory_preset(1);
                }
                if ui.button("🎧 Estúdio Lo-Fi 432Hz").clicked() {
                    self.model.apply_factory_preset(2);
                }
                if ui.button("Espaço Sideral").clicked() {
                    self.model.apply_factory_preset(3);
                }

                ui.separator();
                if ui.button("💾 Salvar").clicked() {
                    self.status_msg = match self.model.save_to_disk() {
                        Ok(m) | Err(m) => m,
                    };
                }
                if ui.button("📂 Carregar").clicked() {
                    self.status_msg = match self.model.load_from_disk() {
                        Ok(m) | Err(m) => m,
                    };
                }
                if ui.button("⏺ Exportar .WAV (15s)").clicked() {
                    match export_wav_session(&self.model, 15, "sessao_delta_asmr.wav") {
                        Ok(_) => self.status_msg = "Exportado: 'sessao_delta_asmr.wav'".to_string(),
                        Err(e) => self.status_msg = format!("Erro WAV: {e}"),
                    }
                }
            });

            ui.add_space(4.0);
            self.draw_analyzer(ui);
            ui.add_space(6.0);

            ui.columns(2, |cols| {
                // COLUNA ESQUERDA: Ondas Cerebrais, 432 Hz e Sub-Drone
                cols[0].group(|ui| {
                    ui.heading("1. Ondas Delta (0,5 – 4,0 Hz)");
                    let mut d_vol = load_f32(&self.model.delta_vol);
                    if ui
                        .add(egui::Slider::new(&mut d_vol, 0.0..=1.0).text("Volume Delta"))
                        .changed()
                    {
                        store_f32(&self.model.delta_vol, d_vol);
                    }
                    let mut d_beat = load_f32(&self.model.delta_beat_hz);
                    if ui
                        .add(egui::Slider::new(&mut d_beat, 0.5..=4.0).text("Pulso Delta (Hz)"))
                        .changed()
                    {
                        store_f32(&self.model.delta_beat_hz, d_beat);
                    }
                    let mut d_carrier = load_f32(&self.model.delta_carrier_hz);
                    if ui
                        .add(egui::Slider::new(&mut d_carrier, 60.0..=300.0).text("Portadora (Hz)"))
                        .changed()
                    {
                        store_f32(&self.model.delta_carrier_hz, d_carrier);
                    }
                    let mut d_mode = self.model.delta_mode.load(Ordering::Relaxed);
                    ui.horizontal(|ui| {
                        ui.label("Modo:");
                        if ui
                            .selectable_value(&mut d_mode, 0, "Binaural (Fones)")
                            .changed()
                            || ui
                                .selectable_value(&mut d_mode, 1, "Isocrônico (Caixas)")
                                .changed()
                        {
                            self.model.delta_mode.store(d_mode, Ordering::Relaxed);
                        }
                    });
                });

                cols[0].add_space(5.0);

                cols[0].group(|ui| {
                    ui.heading("2. Ressonância Schumann (7,83 Hz) & 432 Hz");
                    let mut s_vol = load_f32(&self.model.schumann_vol);
                    if ui
                        .add(egui::Slider::new(&mut s_vol, 0.0..=1.0).text("Volume Schumann"))
                        .changed()
                    {
                        store_f32(&self.model.schumann_vol, s_vol);
                    }
                    let mut s_carrier = load_f32(&self.model.schumann_carrier_hz);
                    if ui
                        .add(
                            egui::Slider::new(&mut s_carrier, 108.0..=432.0)
                                .text("Portadora Schumann"),
                        )
                        .changed()
                    {
                        store_f32(&self.model.schumann_carrier_hz, s_carrier);
                    }
                    let mut s_mode = self.model.schumann_mode.load(Ordering::Relaxed);
                    ui.horizontal(|ui| {
                        ui.label("Modo Schumann:");
                        if ui.selectable_value(&mut s_mode, 0, "Binaural").changed()
                            || ui.selectable_value(&mut s_mode, 1, "Isocrônico").changed()
                        {
                            self.model.schumann_mode.store(s_mode, Ordering::Relaxed);
                        }
                    });

                    ui.separator();
                    let mut f432_vol = load_f32(&self.model.freq432_vol);
                    if ui
                        .add(
                            egui::Slider::new(&mut f432_vol, 0.0..=1.0)
                                .text("Volume Clássica 432 Hz"),
                        )
                        .changed()
                    {
                        store_f32(&self.model.freq432_vol, f432_vol);
                    }
                    let mut sub_oct = self.model.freq432_sub_octave.load(Ordering::Relaxed);
                    if ui
                        .checkbox(&mut sub_oct, "Somar Sub-oitava Harmônica (216 Hz)")
                        .changed()
                    {
                        self.model
                            .freq432_sub_octave
                            .store(sub_oct, Ordering::Relaxed);
                    }
                });

                cols[0].add_space(5.0);

                cols[0].group(|ui| {
                    ui.heading("3. Graves de Fundo (Sub-Drone)");
                    let mut sub_vol = load_f32(&self.model.sub_vol);
                    if ui
                        .add(egui::Slider::new(&mut sub_vol, 0.0..=1.0).text("Volume Sub-Grave"))
                        .changed()
                    {
                        store_f32(&self.model.sub_vol, sub_vol);
                    }
                    let mut sub_f = load_f32(&self.model.sub_freq_hz);
                    if ui
                        .add(egui::Slider::new(&mut sub_f, 36.0..=90.0).text("Fundamental (Hz)"))
                        .changed()
                    {
                        store_f32(&self.model.sub_freq_hz, sub_f);
                    }
                    let mut boost = load_f32(&self.model.sub_harmonic_boost);
                    if ui
                        .add(
                            egui::Slider::new(&mut boost, 0.0..=0.6)
                                .text("2º Harmônico (Psicoacústico)"),
                        )
                        .changed()
                    {
                        store_f32(&self.model.sub_harmonic_boost, boost);
                    }
                });

                // COLUNA DIREITA: Cores de Ruído, Ambiências Polimórficas e Estúdio Lo-Fi
                cols[1].group(|ui| {
                    ui.heading("4. Cores de Ruído (Espectro 1/f^β)");
                    let mut n_vol = load_f32(&self.model.noise_vol);
                    if ui
                        .add(egui::Slider::new(&mut n_vol, 0.0..=1.0).text("Volume do Ruído"))
                        .changed()
                    {
                        store_f32(&self.model.noise_vol, n_vol);
                    }
                    let mut color = self.model.noise_color.load(Ordering::Relaxed);
                    ui.horizontal_wrapped(|ui| {
                        for (idx, nome) in
                            enumerate(["Branco", "Rosa", "Marrom", "Azul", "Violeta"])
                        {
                            if ui.selectable_value(&mut color, idx as u8, nome).changed() {
                                self.model.noise_color.store(color, Ordering::Relaxed);
                            }
                        }
                    });
                });

                cols[1].add_space(5.0);

                cols[1].group(|ui| {
                    ui.heading("5. Ambiência ASMR (Polimórfica)");
                    let mut a_vol = load_f32(&self.model.ambience_vol);
                    if ui
                        .add(egui::Slider::new(&mut a_vol, 0.0..=1.0).text("Volume da Ambiência"))
                        .changed()
                    {
                        store_f32(&self.model.ambience_vol, a_vol);
                    }
                    let mut warmth = load_f32(&self.model.ambience_warmth_hz);
                    if ui
                        .add(
                            egui::Slider::new(&mut warmth, 400.0..=4500.0)
                                .text("Suavidade (Filtro Hz)"),
                        )
                        .changed()
                    {
                        store_f32(&self.model.ambience_warmth_hz, warmth);
                    }
                    let mut amb = self.model.ambience_type.load(Ordering::Relaxed);
                    ui.horizontal_wrapped(|ui| {
                        let opcoes = [
                            "Vento",
                            "Água Corrente",
                            "Riacho Suave",
                            "Chuva",
                            "Espaço Sideral",
                            "Lo-Fi Lounge",
                        ];
                        for (idx, nome) in enumerate(opcoes) {
                            if ui.selectable_value(&mut amb, idx as u8, nome).changed() {
                                self.model.ambience_type.store(amb, Ordering::Relaxed);
                            }
                        }
                    });
                });

                cols[1].add_space(5.0);

                cols[1].group(|ui| {
                    ui.heading("6. Estúdio Lo-Fi Vintage (432 Hz Tuning)");
                    let mut chords = load_f32(&self.model.lofi_chords_vol);
                    if ui
                        .add(
                            egui::Slider::new(&mut chords, 0.0..=1.0)
                                .text("Acordes Rhodes (Dm9-G13-Cmaj9)"),
                        )
                        .changed()
                    {
                        store_f32(&self.model.lofi_chords_vol, chords);
                    }
                    let mut vinyl = load_f32(&self.model.lofi_vinyl_crackle);
                    if ui
                        .add(
                            egui::Slider::new(&mut vinyl, 0.0..=1.0)
                                .text("Agulha e Poeira de Vinil"),
                        )
                        .changed()
                    {
                        store_f32(&self.model.lofi_vinyl_crackle, vinyl);
                    }
                    let mut wow = load_f32(&self.model.lofi_tape_wow);
                    if ui
                        .add(
                            egui::Slider::new(&mut wow, 0.0..=1.0)
                                .text("Fita Cassete (Wow & Flutter)"),
                        )
                        .changed()
                    {
                        store_f32(&self.model.lofi_tape_wow, wow);
                    }
                });
            });

            ui.add_space(5.0);
            ui.separator();

            // Rodapé: Timer de Sono e Telemetria
            ui.horizontal(|ui| {
                ui.checkbox(&mut self.sleep_timer_active, "Timer de Sono (Fade-Out):");
                if ui.button("15m").clicked() {
                    self.sleep_remaining_secs = 900.0;
                    self.sleep_timer_active = true;
                }
                if ui.button("30m").clicked() {
                    self.sleep_remaining_secs = 1800.0;
                    self.sleep_timer_active = true;
                }
                if ui.button("60m").clicked() {
                    self.sleep_remaining_secs = 3600.0;
                    self.sleep_timer_active = true;
                }
                if self.sleep_timer_active {
                    let mins = (self.sleep_remaining_secs as u64) / 60;
                    let secs = (self.sleep_remaining_secs as u64) % 60;
                    ui.label(RichText::new(format!("{mins:02}:{secs:02}")).strong());
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(&self.status_msg).italics().small());
                });
            });
        });

        ctx.request_repaint_after(std::time::Duration::from_millis(16));
    }
}
