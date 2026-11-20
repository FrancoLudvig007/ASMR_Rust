use crate::pyrsutils::prelude::*;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, AtomicUsize, Ordering};

pub const SCOPE_SIZE: usize = 1024;
pub const PRESET_FILE: &str = "meu_preset.cfg";

#[inline]
pub fn store_f32(atom: &AtomicU32, val: f32) {
    atom.store(val.to_bits(), Ordering::Relaxed);
}

#[inline]
pub fn load_f32(atom: &AtomicU32) -> f32 {
    f32::from_bits(atom.load(Ordering::Relaxed))
}

pub struct AudioModel {
    pub playing: AtomicBool,
    pub master_vol: AtomicU32,

    // Camada 1: Ondas Delta (0.5 - 4.0 Hz)
    pub delta_vol: AtomicU32,
    pub delta_carrier_hz: AtomicU32,
    pub delta_beat_hz: AtomicU32,
    pub delta_mode: AtomicU8, // 0 = Binaural (Fones), 1 = Isocrônico (Caixas/Fones)

    // Camada 2: Ressonância Schumann (7.83 Hz)
    pub schumann_vol: AtomicU32,
    pub schumann_carrier_hz: AtomicU32,
    pub schumann_mode: AtomicU8, // 0 = Binaural, 1 = Isocrônico

    // Camada 3: Clássica 432 Hz
    pub freq432_vol: AtomicU32,
    pub freq432_sub_octave: AtomicBool,

    // Camada 4: Graves de Fundo (Sub-Drone Psicoacústico)
    pub sub_vol: AtomicU32,
    pub sub_freq_hz: AtomicU32,
    pub sub_harmonic_boost: AtomicU32,

    // Camada 5: Cores de Ruído
    pub noise_vol: AtomicU32,
    pub noise_color: AtomicU8, // 0=Branco, 1=Rosa, 2=Marrom, 3=Azul, 4=Violeta

    // Camada 6: Ambiência ASMR Polimórfica
    pub ambience_vol: AtomicU32,
    pub ambience_type: AtomicU8, // 0=Vento, 1=Água Corrente, 2=Riacho, 3=Chuva, 4=Espaço, 5=Lo-Fi Lounge
    pub ambience_warmth_hz: AtomicU32,

    // Camada 7: Estúdio Lo-Fi Vintage (Vinil, Fita Cassete & Acordes Rhodes)
    pub lofi_chords_vol: AtomicU32,
    pub lofi_vinyl_crackle: AtomicU32,
    pub lofi_tape_wow: AtomicU32,

    // Buffer Circular Lock-Free do Osciloscópio
    pub scope_l: [AtomicU32; SCOPE_SIZE],
    pub scope_r: [AtomicU32; SCOPE_SIZE],
    pub scope_write_idx: AtomicUsize,
}

impl AudioModel {
    pub fn new() -> Self {
        Self {
            playing: AtomicBool::new(true),
            master_vol: AtomicU32::new(0.75f32.to_bits()),

            delta_vol: AtomicU32::new(0.40f32.to_bits()),
            delta_carrier_hz: AtomicU32::new(108.0f32.to_bits()),
            delta_beat_hz: AtomicU32::new(2.0f32.to_bits()),
            delta_mode: AtomicU8::new(0),

            schumann_vol: AtomicU32::new(0.20f32.to_bits()),
            schumann_carrier_hz: AtomicU32::new(216.0f32.to_bits()),
            schumann_mode: AtomicU8::new(1),

            freq432_vol: AtomicU32::new(0.18f32.to_bits()),
            freq432_sub_octave: AtomicBool::new(true),

            sub_vol: AtomicU32::new(0.28f32.to_bits()),
            sub_freq_hz: AtomicU32::new(54.0f32.to_bits()),
            sub_harmonic_boost: AtomicU32::new(0.22f32.to_bits()),

            noise_vol: AtomicU32::new(0.12f32.to_bits()),
            noise_color: AtomicU8::new(2), // Marrom

            ambience_vol: AtomicU32::new(0.42f32.to_bits()),
            ambience_type: AtomicU8::new(2), // Riacho Suave
            ambience_warmth_hz: AtomicU32::new(1350.0f32.to_bits()),

            lofi_chords_vol: AtomicU32::new(0.0f32.to_bits()),
            lofi_vinyl_crackle: AtomicU32::new(0.0f32.to_bits()),
            lofi_tape_wow: AtomicU32::new(0.25f32.to_bits()),

            scope_l: std::array::from_fn(|_| AtomicU32::new(0.0f32.to_bits())),
            scope_r: std::array::from_fn(|_| AtomicU32::new(0.0f32.to_bits())),
            scope_write_idx: AtomicUsize::new(0),
        }
    }

    pub fn push_scope(&self, left: f32, right: f32) {
        let idx = self.scope_write_idx.fetch_add(1, Ordering::Relaxed) % SCOPE_SIZE;
        store_f32(&self.scope_l[idx], left);
        store_f32(&self.scope_r[idx], right);
    }

    pub fn apply_factory_preset(&self, preset_id: u8) {
        match preset_id {
            // 0: Sono Profundo (Delta 1.5 Hz + Chuva + Ruído Marrom)
            0 => {
                store_f32(&self.delta_vol, 0.55);
                store_f32(&self.delta_carrier_hz, 108.0);
                store_f32(&self.delta_beat_hz, 1.5);
                self.delta_mode.store(0, Ordering::Relaxed);

                store_f32(&self.schumann_vol, 0.10);
                store_f32(&self.freq432_vol, 0.10);
                store_f32(&self.sub_vol, 0.40);
                store_f32(&self.sub_freq_hz, 54.0);

                store_f32(&self.noise_vol, 0.22);
                self.noise_color.store(2, Ordering::Relaxed);

                store_f32(&self.ambience_vol, 0.45);
                self.ambience_type.store(3, Ordering::Relaxed);
                store_f32(&self.ambience_warmth_hz, 1200.0);

                store_f32(&self.lofi_chords_vol, 0.0);
                store_f32(&self.lofi_vinyl_crackle, 0.0);
            }
            // 1: Meditação Schumann & Riacho Laminar
            1 => {
                store_f32(&self.delta_vol, 0.20);
                store_f32(&self.delta_beat_hz, 3.0);

                store_f32(&self.schumann_vol, 0.48);
                store_f32(&self.schumann_carrier_hz, 216.0);
                self.schumann_mode.store(1, Ordering::Relaxed);

                store_f32(&self.freq432_vol, 0.40);
                self.freq432_sub_octave.store(true, Ordering::Relaxed);

                store_f32(&self.sub_vol, 0.25);
                store_f32(&self.noise_vol, 0.08);
                self.noise_color.store(1, Ordering::Relaxed);

                store_f32(&self.ambience_vol, 0.50);
                self.ambience_type.store(2, Ordering::Relaxed);
                store_f32(&self.ambience_warmth_hz, 1150.0);

                store_f32(&self.lofi_chords_vol, 0.0);
                store_f32(&self.lofi_vinyl_crackle, 0.0);
            }
            // 2: Estúdio Lo-Fi Nocturne (Acordes Rhodes 432 Hz + Vinil + Chuva Suave + Delta)
            2 => {
                store_f32(&self.delta_vol, 0.30);
                store_f32(&self.delta_carrier_hz, 108.0);
                store_f32(&self.delta_beat_hz, 2.5);

                store_f32(&self.schumann_vol, 0.20);
                store_f32(&self.freq432_vol, 0.15);
                store_f32(&self.sub_vol, 0.32);
                store_f32(&self.sub_freq_hz, 54.0);

                store_f32(&self.noise_vol, 0.06);
                self.noise_color.store(1, Ordering::Relaxed);

                store_f32(&self.ambience_vol, 0.35);
                self.ambience_type.store(5, Ordering::Relaxed); // Lo-Fi Lounge
                store_f32(&self.ambience_warmth_hz, 1450.0);

                store_f32(&self.lofi_chords_vol, 0.48);
                store_f32(&self.lofi_vinyl_crackle, 0.35);
                store_f32(&self.lofi_tape_wow, 0.38);
            }
            // 3: Imersão Cósmica (Espaço Sideral + Sub-Drone 42 Hz)
            _ => {
                store_f32(&self.delta_vol, 0.45);
                store_f32(&self.delta_carrier_hz, 96.0);
                store_f32(&self.delta_beat_hz, 0.8);

                store_f32(&self.schumann_vol, 0.30);
                store_f32(&self.freq432_vol, 0.25);

                store_f32(&self.sub_vol, 0.50);
                store_f32(&self.sub_freq_hz, 42.0);

                store_f32(&self.noise_vol, 0.10);
                self.noise_color.store(1, Ordering::Relaxed);

                store_f32(&self.ambience_vol, 0.55);
                self.ambience_type.store(4, Ordering::Relaxed);
                store_f32(&self.ambience_warmth_hz, 1600.0);

                store_f32(&self.lofi_chords_vol, 0.0);
                store_f32(&self.lofi_vinyl_crackle, 0.0);
            }
        }
    }

    pub fn save_to_disk(&self) -> Result<String, String> {
        let valores = [
            load_f32(&self.master_vol).to_string(),
            load_f32(&self.delta_vol).to_string(),
            load_f32(&self.delta_carrier_hz).to_string(),
            load_f32(&self.delta_beat_hz).to_string(),
            self.delta_mode.load(Ordering::Relaxed).to_string(),
            load_f32(&self.schumann_vol).to_string(),
            load_f32(&self.schumann_carrier_hz).to_string(),
            self.schumann_mode.load(Ordering::Relaxed).to_string(),
            load_f32(&self.freq432_vol).to_string(),
            load_f32(&self.sub_vol).to_string(),
            load_f32(&self.sub_freq_hz).to_string(),
            load_f32(&self.noise_vol).to_string(),
            self.noise_color.load(Ordering::Relaxed).to_string(),
            load_f32(&self.ambience_vol).to_string(),
            self.ambience_type.load(Ordering::Relaxed).to_string(),
            load_f32(&self.ambience_warmth_hz).to_string(),
            load_f32(&self.lofi_chords_vol).to_string(),
            load_f32(&self.lofi_vinyl_crackle).to_string(),
            load_f32(&self.lofi_tape_wow).to_string(),
        ];
        let conteudo = join(";", valores);
        write_text(PRESET_FILE, conteudo)
            .map(|_| format!("Preset salvo em '{PRESET_FILE}'"))
            .map_err(|e| format!("Erro ao salvar preset: {e}"))
    }

    pub fn load_from_disk(&self) -> Result<String, String> {
        if !exists(PRESET_FILE) {
            return Err(format!("Arquivo '{PRESET_FILE}' ainda não existe"));
        }
        let texto = read_text(PRESET_FILE).map_err(|e| e.to_string())?;
        let partes = split(strip(&texto), ";");
        if len(&partes) < 19 {
            return Err("Formato de preset inválido ou antigo".to_string());
        }
        if let Ok(v) = partes[0].parse::<f32>() {
            store_f32(&self.master_vol, v);
        }
        if let Ok(v) = partes[1].parse::<f32>() {
            store_f32(&self.delta_vol, v);
        }
        if let Ok(v) = partes[2].parse::<f32>() {
            store_f32(&self.delta_carrier_hz, v);
        }
        if let Ok(v) = partes[3].parse::<f32>() {
            store_f32(&self.delta_beat_hz, v);
        }
        if let Ok(v) = partes[4].parse::<u8>() {
            self.delta_mode.store(v, Ordering::Relaxed);
        }
        if let Ok(v) = partes[5].parse::<f32>() {
            store_f32(&self.schumann_vol, v);
        }
        if let Ok(v) = partes[6].parse::<f32>() {
            store_f32(&self.schumann_carrier_hz, v);
        }
        if let Ok(v) = partes[7].parse::<u8>() {
            self.schumann_mode.store(v, Ordering::Relaxed);
        }
        if let Ok(v) = partes[8].parse::<f32>() {
            store_f32(&self.freq432_vol, v);
        }
        if let Ok(v) = partes[9].parse::<f32>() {
            store_f32(&self.sub_vol, v);
        }
        if let Ok(v) = partes[10].parse::<f32>() {
            store_f32(&self.sub_freq_hz, v);
        }
        if let Ok(v) = partes[11].parse::<f32>() {
            store_f32(&self.noise_vol, v);
        }
        if let Ok(v) = partes[12].parse::<u8>() {
            self.noise_color.store(v, Ordering::Relaxed);
        }
        if let Ok(v) = partes[13].parse::<f32>() {
            store_f32(&self.ambience_vol, v);
        }
        if let Ok(v) = partes[14].parse::<u8>() {
            self.ambience_type.store(v, Ordering::Relaxed);
        }
        if let Ok(v) = partes[15].parse::<f32>() {
            store_f32(&self.ambience_warmth_hz, v);
        }
        if let Ok(v) = partes[16].parse::<f32>() {
            store_f32(&self.lofi_chords_vol, v);
        }
        if let Ok(v) = partes[17].parse::<f32>() {
            store_f32(&self.lofi_vinyl_crackle, v);
        }
        if let Ok(v) = partes[18].parse::<f32>() {
            store_f32(&self.lofi_tape_wow, v);
        }

        Ok(format!("Preset carregado de '{PRESET_FILE}'"))
    }
}
