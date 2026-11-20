use crate::class;
use crate::model::{load_f32, AudioModel};
use crate::pyrsutils::prelude::*;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::f32::consts::TAU;
use std::fs::File;
use std::io::{BufWriter, Write};
use std::sync::atomic::Ordering;
use std::sync::Arc;

// ============================================================================
// 1. Classes Base e Derivadas (Herança via macro `class!` da pyrsutils.rs)
// ============================================================================

class! {
    struct Oscillator {
        phase: f32
    }
    impl {
        fn next_sin(&mut self, freq_hz: f32, sr: f32) -> f32 {
            let out = self.phase.sin();
            self.phase = (self.phase + TAU * freq_hz / sr) % TAU;
            out
        }

        fn next_rhodes_tone(&mut self, freq_hz: f32, sr: f32) -> f32 {
            // Timbre elétrico suave estilo Fender Rhodes (fundamental + 2º e 3º harmônicos suaves)
            let f1 = self.phase.sin();
            let f2 = (self.phase * 2.0).sin() * 0.22;
            let f3 = (self.phase * 3.0).sin() * 0.06;
            self.phase = (self.phase + TAU * freq_hz / sr) % TAU;
            f1 + f2 + f3
        }

        fn next_isochronic_env(&mut self, beat_hz: f32, sr: f32) -> f32 {
            let env = 0.5 * (1.0 - self.phase.cos());
            self.phase = (self.phase + TAU * beat_hz / sr) % TAU;
            env
        }
    }
}

// Herança: `SubDrone` herda de `Oscillator`
class! {
    struct SubDrone : Oscillator {
        phase_h2: f32,
        phase_lfo: f32
    }
    impl {
        fn sample(&mut self, freq: f32, harmonic_boost: f32, sr: f32) -> f32 {
            let fundamental = self.next_sin(freq, sr);
            let h2 = self.phase_h2.sin();
            self.phase_h2 = (self.phase_h2 + TAU * (freq * 2.0) / sr) % TAU;

            let breath = 0.85 + 0.15 * self.phase_lfo.sin();
            self.phase_lfo = (self.phase_lfo + TAU * 0.08 / sr) % TAU;

            (fundamental + harmonic_boost * h2) * breath
        }
    }
}

class! {
    struct FilterBase {
        z1: f32
    }
    impl {
        fn lowpass_1pole(&mut self, input: f32, cutoff_hz: f32, sr: f32) -> f32 {
            let rc = 1.0 / (TAU * cutoff_hz.max(0.05));
            let dt = 1.0 / sr;
            let alpha = dt / (rc + dt);
            self.z1 += alpha * (input - self.z1);
            self.z1
        }
    }
}

// Herança: `BiquadFilter` herda `z1` e `lowpass_1pole` de `FilterBase`
class! {
    struct BiquadFilter : FilterBase {
        z2: f32
    }
    impl {
        fn bandpass(&mut self, input: f32, cutoff_hz: f32, q: f32, sr: f32) -> f32 {
            let f = (cutoff_hz / sr).clamp(0.001, 0.45);
            let omega = TAU * f;
            let (sin_w, cos_w) = omega.sin_cos();
            let alpha = sin_w / (2.0 * q.max(0.4));

            let b0 = alpha;
            let b2 = -alpha;
            let a0 = 1.0 + alpha;
            let a1 = -2.0 * cos_w;
            let a2 = 1.0 - alpha;

            let out = (b0 / a0) * input + self.z1;
            self.z1 = -(a1 / a0) * out + self.z2;
            self.z2 = (b2 / a0) * input - (a2 / a0) * out;
            out
        }

        fn lowpass_biquad(&mut self, input: f32, cutoff_hz: f32, q: f32, sr: f32) -> f32 {
            let f = (cutoff_hz / sr).clamp(0.002, 0.45);
            let omega = TAU * f;
            let (sin_w, cos_w) = omega.sin_cos();
            let alpha = sin_w / (2.0 * q.max(0.5));

            let b1 = 1.0 - cos_w;
            let b0 = b1 * 0.5;
            let b2 = b0;
            let a0 = 1.0 + alpha;
            let a1 = -2.0 * cos_w;
            let a2 = 1.0 - alpha;

            let out = (b0 / a0) * input + self.z1;
            self.z1 = (b1 / a0) * input - (a1 / a0) * out + self.z2;
            self.z2 = (b2 / a0) * input - (a2 / a0) * out;
            out
        }
    }
}

fn new_biquad() -> BiquadFilter {
    BiquadFilter::new(FilterBase::new(0.0), 0.0)
}

class! {
    struct NoiseSynth {
        b0: f32,
        b1: f32,
        b2: f32,
        brown_state: f32,
        prev_white: f32,
        prev_pink: f32
    }
    impl {
        fn white(&mut self) -> f32 {
            (random() as f32) * 2.0 - 1.0
        }

        fn pink(&mut self) -> f32 {
            let w = self.white();
            self.b0 = 0.99765 * self.b0 + w * 0.0990460;
            self.b1 = 0.96300 * self.b1 + w * 0.2965164;
            self.b2 = 0.57000 * self.b2 + w * 1.0526913;
            (self.b0 + self.b1 + self.b2 + w * 0.1848) * 0.18
        }

        fn brown(&mut self) -> f32 {
            let w = self.white();
            self.brown_state = (self.brown_state * 0.985 + w * 0.035).clamp(-1.0, 1.0);
            self.brown_state
        }

        fn blue(&mut self) -> f32 {
            let p = self.pink();
            let diff = (p - self.prev_pink) * 1.6;
            self.prev_pink = p;
            diff.clamp(-1.0, 1.0)
        }

        fn violet(&mut self) -> f32 {
            let w = self.white();
            let diff = (w - self.prev_white) * 0.5;
            self.prev_white = w;
            diff.clamp(-1.0, 1.0)
        }

        fn sample_color(&mut self, color_id: u8) -> f32 {
            match color_id {
                0 => self.white() * 0.35,
                1 => self.pink() * 0.75,
                2 => self.brown() * 0.95,
                3 => self.blue() * 0.55,
                _ => self.violet() * 0.35,
            }
        }
    }
}

fn new_noise() -> NoiseSynth {
    NoiseSynth::new(0.0, 0.0, 0.0, 0.0, 0.0, 0.0)
}

// ============================================================================
// 2. Polimorfismo Real: Interface `AmbienceGenerator` e 6 Implementações
// ============================================================================

pub trait AmbienceGenerator: Send {
    fn sample_stereo(&mut self, sr: f32) -> (f32, f32);
}

// 2.1 Vento Atmosférico
class! {
    struct WindAmbience {
        noise_l: NoiseSynth,
        noise_r: NoiseSynth,
        flt_l: BiquadFilter,
        flt_r: BiquadFilter,
        drift: FilterBase,
        lfo1: Oscillator,
        lfo2: Oscillator
    }
    impl {}
}

impl AmbienceGenerator for WindAmbience {
    fn sample_stereo(&mut self, sr: f32) -> (f32, f32) {
        let mod1 = self.lfo1.next_sin(0.09, sr);
        let mod2 = self.lfo2.next_sin(0.037, sr);
        let gust = self.drift.lowpass_1pole(self.noise_l.brown(), 0.4, sr);

        let cutoff_l = (240.0 + 140.0 * mod1 + 90.0 * gust).clamp(90.0, 800.0);
        let cutoff_r = (260.0 - 120.0 * mod1 + 110.0 * mod2).clamp(90.0, 800.0);

        let env = 0.65 + 0.35 * (0.5 * (mod1 + mod2));
        let l = self.flt_l.bandpass(self.noise_l.pink(), cutoff_l, 3.2, sr) * env * 2.4;
        let r = self.flt_r.bandpass(self.noise_r.pink(), cutoff_r, 3.2, sr) * env * 2.4;
        (l, r)
    }
}

// 2.2 Água Corrente (Fluxo Hidrodinâmico Profundo, sem LFO periódico)
class! {
    struct FlowingWaterAmbience {
        noise_l: NoiseSynth,
        noise_r: NoiseSynth,
        flt_body_l: BiquadFilter,
        flt_body_r: BiquadFilter,
        flt_crest_l: BiquadFilter,
        flt_crest_r: BiquadFilter,
        drift_l: FilterBase,
        drift_r: FilterBase
    }
    impl {}
}

impl AmbienceGenerator for FlowingWaterAmbience {
    fn sample_stereo(&mut self, sr: f32) -> (f32, f32) {
        let deep_l = self.noise_l.brown();
        let deep_r = self.noise_r.brown();
        let surf_l = self.noise_l.pink();
        let surf_r = self.noise_r.pink();

        let vortex_l = self.drift_l.lowpass_1pole(deep_l, 0.6, sr);
        let vortex_r = self.drift_r.lowpass_1pole(deep_r, 0.6, sr);

        let body_l = self
            .flt_body_l
            .bandpass(deep_l, 210.0 + 55.0 * vortex_l, 0.9, sr);
        let body_r = self
            .flt_body_r
            .bandpass(deep_r, 225.0 + 55.0 * vortex_r, 0.9, sr);

        let crest_l = self
            .flt_crest_l
            .bandpass(surf_l, 480.0 + 130.0 * vortex_r, 1.3, sr);
        let crest_r = self
            .flt_crest_r
            .bandpass(surf_r, 510.0 + 130.0 * vortex_l, 1.3, sr);

        let l = body_l * 1.3 + crest_l * 0.75 + deep_l * 0.25;
        let r = body_r * 1.3 + crest_r * 0.75 + deep_r * 0.25;
        (l, r)
    }
}

// 2.3 Riacho Suave (Escoamento Laminar de 4 Formantes Líquidos)
class! {
    struct GentleBrookAmbience {
        noise_l: NoiseSynth,
        noise_r: NoiseSynth,
        flt_l1: BiquadFilter,
        flt_l2: BiquadFilter,
        flt_r1: BiquadFilter,
        flt_r2: BiquadFilter,
        drift1: FilterBase,
        drift2: FilterBase,
        drift3: FilterBase,
        drift4: FilterBase
    }
    impl {}
}

impl AmbienceGenerator for GentleBrookAmbience {
    fn sample_stereo(&mut self, sr: f32) -> (f32, f32) {
        let src_l = self.noise_l.pink() * 0.7 + self.noise_l.brown() * 0.3;
        let src_r = self.noise_r.pink() * 0.7 + self.noise_r.brown() * 0.3;

        let w1 = self.drift1.lowpass_1pole(self.noise_l.brown(), 1.6, sr);
        let w2 = self.drift2.lowpass_1pole(self.noise_r.brown(), 2.1, sr);
        let w3 = self.drift3.lowpass_1pole(self.noise_l.pink(), 1.2, sr);
        let w4 = self.drift4.lowpass_1pole(self.noise_r.pink(), 1.9, sr);

        let f1 = (320.0 + 110.0 * w1).clamp(180.0, 650.0);
        let f2 = (540.0 + 170.0 * w2).clamp(320.0, 920.0);
        let f3 = (350.0 + 120.0 * w3).clamp(190.0, 680.0);
        let f4 = (590.0 + 180.0 * w4).clamp(340.0, 960.0);

        let env_l = 0.75 + 0.25 * w3;
        let env_r = 0.75 + 0.25 * w1;

        let murmur_l = (self.flt_l1.bandpass(src_l, f1, 2.6, sr)
            + self.flt_l2.bandpass(src_l, f2, 3.0, sr) * 0.7)
            * env_l;
        let murmur_r = (self.flt_r1.bandpass(src_r, f3, 2.6, sr)
            + self.flt_r2.bandpass(src_r, f4, 3.0, sr) * 0.7)
            * env_r;

        let bed_l = self.noise_l.brown() * 0.22;
        let bed_r = self.noise_r.brown() * 0.22;

        (murmur_l * 1.45 + bed_l, murmur_r * 1.45 + bed_r)
    }
}

// 2.4 Chuva Aveludada
class! {
    struct RainAmbience {
        noise_l: NoiseSynth,
        noise_r: NoiseSynth,
        flt_bed_l: BiquadFilter,
        flt_bed_r: BiquadFilter,
        flt_drop: BiquadFilter,
        drop_env: f32,
        drop_freq: f32
    }
    impl {}
}

impl AmbienceGenerator for RainAmbience {
    fn sample_stereo(&mut self, sr: f32) -> (f32, f32) {
        let p_l = self.noise_l.pink();
        let p_r = self.noise_r.pink();
        let bed_l = self.flt_bed_l.lowpass_biquad(p_l, 1350.0, 0.707, sr) * 0.65;
        let bed_r = self.flt_bed_r.lowpass_biquad(p_r, 1400.0, 0.707, sr) * 0.65;

        if self.drop_env < 0.005 && (random() as f32) < (32.0 / sr) {
            self.drop_env = 1.0;
            self.drop_freq = 1100.0 + (random() as f32) * 1600.0;
        }

        let mut drop_sig = 0.0;
        if self.drop_env >= 0.005 {
            let impulse = self.noise_l.pink() * self.drop_env;
            drop_sig = self.flt_drop.bandpass(impulse, self.drop_freq, 3.0, sr) * 0.9;
            self.drop_env *= (-140.0 / sr).exp();
        }

        (bed_l + drop_sig, bed_r + drop_sig * 0.85)
    }
}

// 2.5 Espaço Sideral
class! {
    struct DeepSpaceAmbience {
        noise_l: NoiseSynth,
        noise_r: NoiseSynth,
        flt_l: BiquadFilter,
        flt_r: BiquadFilter,
        lfo1: Oscillator,
        lfo2: Oscillator,
        osc1: Oscillator,
        osc2: Oscillator,
        shimmer: Oscillator
    }
    impl {}
}

impl AmbienceGenerator for DeepSpaceAmbience {
    fn sample_stereo(&mut self, sr: f32) -> (f32, f32) {
        let sweep = 0.5 + 0.5 * self.lfo1.next_sin(0.035, sr);
        let shimmer_mod = 0.5 + 0.5 * self.lfo2.next_sin(0.09, sr);

        let drone_54 = self.osc1.next_sin(54.0, sr);
        let drone_81 = self.osc2.next_sin(81.02, sr);
        let shim = self.shimmer.next_sin(864.0, sr) * shimmer_mod * 0.07;

        let dust_l = self
            .flt_l
            .bandpass(self.noise_l.pink(), 160.0 + 320.0 * sweep, 4.5, sr);
        let dust_r =
            self.flt_r
                .bandpass(self.noise_r.pink(), 190.0 + 300.0 * (1.0 - sweep), 4.5, sr);

        let l = drone_54 * 0.45 + drone_81 * 0.25 + dust_l * 1.3 + shim;
        let r = drone_54 * 0.35 + drone_81 * 0.40 + dust_r * 1.3 + shim;
        (l, r)
    }
}

// 2.6 Ambiência Lo-Fi Lounge (Chuva Abafada de Janela + Sala Quente + Vinil Suave)
class! {
    struct LoFiLoungeAmbience {
        noise_l: NoiseSynth,
        noise_r: NoiseSynth,
        room_flt_l: BiquadFilter,
        room_flt_r: BiquadFilter,
        crackle_flt: BiquadFilter,
        pop_env: f32
    }
    impl {}
}

impl AmbienceGenerator for LoFiLoungeAmbience {
    fn sample_stereo(&mut self, sr: f32) -> (f32, f32) {
        // Chuva abafada vista de dentro do quarto (corte em 650 Hz)
        let rain_l = self
            .room_flt_l
            .lowpass_biquad(self.noise_l.pink(), 650.0, 0.8, sr)
            * 0.8;
        let rain_r = self
            .room_flt_r
            .lowpass_biquad(self.noise_r.pink(), 680.0, 0.8, sr)
            * 0.8;

        // Micro-estalos de poeira de disco de vinil
        if self.pop_env < 0.002 && (random() as f32) < (18.0 / sr) {
            self.pop_env = 0.5 + 0.5 * (random() as f32);
        }
        let mut vinyl_pop = 0.0;
        if self.pop_env >= 0.002 {
            vinyl_pop =
                self.crackle_flt
                    .bandpass(self.noise_l.white() * self.pop_env, 2800.0, 2.5, sr);
            self.pop_env *= (-450.0 / sr).exp();
        }

        (rain_l + vinyl_pop * 0.45, rain_r + vinyl_pop * 0.40)
    }
}

// ============================================================================
// 3. Sintetizador Dedicado Lo-Fi Vintage (Acordes Rhodes 432 Hz, Fita e Vinil)
// ============================================================================

class! {
    struct LoFiVintageSynth {
        osc_v1: Oscillator,
        osc_v2: Oscillator,
        osc_v3: Oscillator,
        osc_v4: Oscillator,
        wow_lfo: Oscillator,
        flutter_lfo: Oscillator,
        tremolo_lfo: Oscillator,
        chord_timer: f32,
        chord_idx: usize,
        lp_l: BiquadFilter,
        lp_r: BiquadFilter,
        vinyl_flt: BiquadFilter,
        noise: NoiseSynth,
        crackle_env: f32
    }
    impl {
        fn process(
            &mut self,
            chords_vol: f32,
            vinyl_vol: f32,
            tape_wow: f32,
            sr: f32,
        ) -> (f32, f32) {
            let mut out_l = 0.0f32;
            let mut out_r = 0.0f32;

            // 1. Acordes Elétricos Rhodes (Progressão Dm9 -> G13 -> Cmaj9 -> Am9 afinada em 432 Hz)
            if chords_vol > 0.001 {
                // Troca suave de acorde a cada 4 segundos (com envelope de ataque/release macio)
                let chord_duration = 4.0f32;
                self.chord_timer += 1.0 / sr;
                if self.chord_timer >= chord_duration {
                    self.chord_timer -= chord_duration;
                    self.chord_idx = (self.chord_idx + 1) & 3;
                }

                // Envelope suave de transição entre acordes (evita cliques na troca harmônica)
                let norm_t = self.chord_timer / chord_duration;
                let env = (norm_t * TAU * 0.5).sin().powf(0.45);

                // Instabilidade de fita cassete (Wow = 0.55 Hz lento, Flutter = 6.2 Hz rápido)
                let wow = self.wow_lfo.next_sin(0.55, sr) * (0.006 * tape_wow);
                let flutter = self.flutter_lfo.next_sin(6.2, sr) * (0.0015 * tape_wow);
                let pitch_mod = 1.0 + wow + flutter;

                // Frequências exatas na escala de A4 = 432 Hz
                let notes: [f32; 4] = match self.chord_idx {
                    0 => [144.16, 172.99, 256.87, 324.00], // Dm9 (D3, F3, C4, E4)
                    1 => [192.43, 242.45, 288.33, 324.00], // G13 (G3, B3, D4, E4)
                    2 => [128.43, 161.82, 242.45, 288.33], // Cmaj9 (C3, E3, B3, D4)
                    _ => [108.00, 161.82, 192.43, 242.45], // Am9 (A2, E3, G3, B3)
                };

                let v1 = self.osc_v1.next_rhodes_tone(notes[0] * pitch_mod, sr);
                let v2 = self.osc_v2.next_rhodes_tone(notes[1] * pitch_mod, sr);
                let v3 = self.osc_v3.next_rhodes_tone(notes[2] * pitch_mod, sr);
                let v4 = self.osc_v4.next_rhodes_tone(notes[3] * pitch_mod, sr);

                let chord_sig = (v1 * 0.30 + v2 * 0.25 + v3 * 0.25 + v4 * 0.20) * env;

                // Autopanner / Tremolo estéreo estilo Fender Rhodes Suitcase
                let pan = self.tremolo_lfo.next_sin(1.8, sr) * 0.30;
                let warm_l = self.lp_l.lowpass_biquad(chord_sig * (0.75 + pan), 950.0, 0.85, sr);
                let warm_r = self.lp_r.lowpass_biquad(chord_sig * (0.75 - pan), 950.0, 0.85, sr);

                out_l += warm_l * chords_vol * 0.85;
                out_r += warm_r * chords_vol * 0.85;
            }

            // 2. Gerador Físico de Agulha em Disco de Vinil (Hiss de sulco + Estalos de poeira)
            if vinyl_vol > 0.001 {
                let groove_hiss = self.noise.pink() * 0.06;
                if self.crackle_env < 0.001 && (random() as f32) < (28.0 / sr) {
                    self.crackle_env = 0.4 + 0.6 * (random() as f32);
                }
                let mut crackle = 0.0;
                if self.crackle_env >= 0.001 {
                    let raw_pop = self.noise.white() * self.crackle_env;
                    crackle = self.vinyl_flt.bandpass(raw_pop, 3100.0, 3.0, sr) * 0.9;
                    self.crackle_env *= (-550.0 / sr).exp();
                }
                let v_sig = (groove_hiss + crackle) * vinyl_vol;
                out_l += v_sig;
                out_r += v_sig * 0.92;
            }

            (out_l, out_r)
        }
    }
}

// ============================================================================
// 4. Controlador Principal de DSP e Gerenciamento de Áudio
// ============================================================================

pub struct DspController {
    sr: f32,
    delta_l: Oscillator,
    delta_r: Oscillator,
    delta_iso: Oscillator,
    schumann_l: Oscillator,
    schumann_r: Oscillator,
    schumann_iso: Oscillator,
    osc_432: Oscillator,
    osc_216: Oscillator,
    sub_drone: SubDrone,
    noise_layer: NoiseSynth,
    // Coleção Polimórfica de Ambiências
    ambiences: Vec<Box<dyn AmbienceGenerator>>,
    warmth_l: BiquadFilter,
    warmth_r: BiquadFilter,
    lofi_synth: LoFiVintageSynth,

    // Suavizadores exponenciais de parâmetros
    smooth_master: f32,
    smooth_delta_vol: f32,
    smooth_schumann_vol: f32,
    smooth_432_vol: f32,
    smooth_sub_vol: f32,
    smooth_noise_vol: f32,
    smooth_ambience_vol: f32,
    smooth_warmth_hz: f32,
    smooth_lofi_chords: f32,
    smooth_lofi_vinyl: f32,
    smooth_lofi_wow: f32,
    scope_decimator: usize,
}

impl DspController {
    pub fn new(sample_rate: f32) -> Self {
        let ambiences: Vec<Box<dyn AmbienceGenerator>> = vec![
            Box::new(WindAmbience::new(
                new_noise(),
                new_noise(),
                new_biquad(),
                new_biquad(),
                FilterBase::new(0.0),
                Oscillator::new(0.0),
                Oscillator::new(1.5),
            )),
            Box::new(FlowingWaterAmbience::new(
                new_noise(),
                new_noise(),
                new_biquad(),
                new_biquad(),
                new_biquad(),
                new_biquad(),
                FilterBase::new(0.0),
                FilterBase::new(0.0),
            )),
            Box::new(GentleBrookAmbience::new(
                new_noise(),
                new_noise(),
                new_biquad(),
                new_biquad(),
                new_biquad(),
                new_biquad(),
                FilterBase::new(0.0),
                FilterBase::new(0.0),
                FilterBase::new(0.0),
                FilterBase::new(0.0),
            )),
            Box::new(RainAmbience::new(
                new_noise(),
                new_noise(),
                new_biquad(),
                new_biquad(),
                new_biquad(),
                0.0,
                1400.0,
            )),
            Box::new(DeepSpaceAmbience::new(
                new_noise(),
                new_noise(),
                new_biquad(),
                new_biquad(),
                Oscillator::new(0.0),
                Oscillator::new(1.2),
                Oscillator::new(0.0),
                Oscillator::new(0.0),
                Oscillator::new(0.0),
            )),
            Box::new(LoFiLoungeAmbience::new(
                new_noise(),
                new_noise(),
                new_biquad(),
                new_biquad(),
                new_biquad(),
                0.0,
            )),
        ];

        Self {
            sr: sample_rate,
            delta_l: Oscillator::new(0.0),
            delta_r: Oscillator::new(0.0),
            delta_iso: Oscillator::new(0.0),
            schumann_l: Oscillator::new(0.0),
            schumann_r: Oscillator::new(0.0),
            schumann_iso: Oscillator::new(0.0),
            osc_432: Oscillator::new(0.0),
            osc_216: Oscillator::new(0.0),
            sub_drone: SubDrone::new(Oscillator::new(0.0), 0.0, 0.0),
            noise_layer: new_noise(),
            ambiences,
            warmth_l: new_biquad(),
            warmth_r: new_biquad(),
            lofi_synth: LoFiVintageSynth::new(
                Oscillator::new(0.0),
                Oscillator::new(0.0),
                Oscillator::new(0.0),
                Oscillator::new(0.0),
                Oscillator::new(0.0),
                Oscillator::new(0.0),
                Oscillator::new(0.0),
                0.0,
                0,
                new_biquad(),
                new_biquad(),
                new_biquad(),
                new_noise(),
                0.0,
            ),
            smooth_master: 0.0,
            smooth_delta_vol: 0.0,
            smooth_schumann_vol: 0.0,
            smooth_432_vol: 0.0,
            smooth_sub_vol: 0.0,
            smooth_noise_vol: 0.0,
            smooth_ambience_vol: 0.0,
            smooth_warmth_hz: 1350.0,
            smooth_lofi_chords: 0.0,
            smooth_lofi_vinyl: 0.0,
            smooth_lofi_wow: 0.25,
            scope_decimator: 0,
        }
    }

    #[inline]
    pub fn next_stereo_frame(&mut self, model: &AudioModel, push_to_scope: bool) -> (f32, f32) {
        if !model.playing.load(Ordering::Relaxed) && push_to_scope {
            self.smooth_master *= 0.995;
            if self.smooth_master < 1e-4 {
                return (0.0, 0.0);
            }
        }

        let alpha = 0.0015;
        let target_master = if model.playing.load(Ordering::Relaxed) || !push_to_scope {
            load_f32(&model.master_vol)
        } else {
            0.0
        };
        self.smooth_master += (target_master - self.smooth_master) * alpha;
        self.smooth_delta_vol += (load_f32(&model.delta_vol) - self.smooth_delta_vol) * alpha;
        self.smooth_schumann_vol +=
            (load_f32(&model.schumann_vol) - self.smooth_schumann_vol) * alpha;
        self.smooth_432_vol += (load_f32(&model.freq432_vol) - self.smooth_432_vol) * alpha;
        self.smooth_sub_vol += (load_f32(&model.sub_vol) - self.smooth_sub_vol) * alpha;
        self.smooth_noise_vol += (load_f32(&model.noise_vol) - self.smooth_noise_vol) * alpha;
        self.smooth_ambience_vol +=
            (load_f32(&model.ambience_vol) - self.smooth_ambience_vol) * alpha;
        self.smooth_warmth_hz +=
            (load_f32(&model.ambience_warmth_hz) - self.smooth_warmth_hz) * alpha;
        self.smooth_lofi_chords +=
            (load_f32(&model.lofi_chords_vol) - self.smooth_lofi_chords) * alpha;
        self.smooth_lofi_vinyl +=
            (load_f32(&model.lofi_vinyl_crackle) - self.smooth_lofi_vinyl) * alpha;
        self.smooth_lofi_wow += (load_f32(&model.lofi_tape_wow) - self.smooth_lofi_wow) * alpha;

        // 1. Camada Delta
        let d_carrier = load_f32(&model.delta_carrier_hz);
        let d_beat = load_f32(&model.delta_beat_hz);
        let (delta_l, delta_r) = if model.delta_mode.load(Ordering::Relaxed) == 0 {
            let l = self.delta_l.next_sin(d_carrier - d_beat * 0.5, self.sr);
            let r = self.delta_r.next_sin(d_carrier + d_beat * 0.5, self.sr);
            (l * self.smooth_delta_vol, r * self.smooth_delta_vol)
        } else {
            let c = self.delta_l.next_sin(d_carrier, self.sr);
            let env = self.delta_iso.next_isochronic_env(d_beat, self.sr);
            let m = c * env * self.smooth_delta_vol;
            (m, m)
        };

        // 2. Camada Ressonância Schumann (7.83 Hz)
        let s_carrier = load_f32(&model.schumann_carrier_hz);
        let s_beat = 7.83_f32;
        let (sch_l, sch_r) = if model.schumann_mode.load(Ordering::Relaxed) == 0 {
            let l = self.schumann_l.next_sin(s_carrier - s_beat * 0.5, self.sr);
            let r = self.schumann_r.next_sin(s_carrier + s_beat * 0.5, self.sr);
            (l * self.smooth_schumann_vol, r * self.smooth_schumann_vol)
        } else {
            let c = self.schumann_l.next_sin(s_carrier, self.sr);
            let env = self.schumann_iso.next_isochronic_env(s_beat, self.sr);
            let m = c * env * self.smooth_schumann_vol;
            (m, m)
        };

        // 3. Camada Clássica 432 Hz
        let mut sig_432 = self.osc_432.next_sin(432.0, self.sr);
        if model.freq432_sub_octave.load(Ordering::Relaxed) {
            sig_432 = sig_432 * 0.7 + self.osc_216.next_sin(216.0, self.sr) * 0.3;
        }
        sig_432 *= self.smooth_432_vol;

        // 4. Camada Sub-Drone
        let sub_f = load_f32(&model.sub_freq_hz);
        let sub_h2 = load_f32(&model.sub_harmonic_boost);
        let sig_sub = self.sub_drone.sample(sub_f, sub_h2, self.sr) * self.smooth_sub_vol;

        // 5. Camada de Cores de Ruído
        let n_color = model.noise_color.load(Ordering::Relaxed);
        let sig_noise = self.noise_layer.sample_color(n_color) * self.smooth_noise_vol;

        // 6. Despacho Polimórfico da Ambiência Selecionada
        let amb_idx = (model.ambience_type.load(Ordering::Relaxed) as usize) % self.ambiences.len();
        let (raw_amb_l, raw_amb_r) = self.ambiences[amb_idx].sample_stereo(self.sr);
        let amb_l = self
            .warmth_l
            .lowpass_biquad(raw_amb_l, self.smooth_warmth_hz, 0.707, self.sr)
            * self.smooth_ambience_vol;
        let amb_r = self
            .warmth_r
            .lowpass_biquad(raw_amb_r, self.smooth_warmth_hz, 0.707, self.sr)
            * self.smooth_ambience_vol;

        // 7. Camada Lo-Fi Vintage (Acordes Rhodes 432 Hz, Fita e Vinil)
        let (lofi_l, lofi_r) = self.lofi_synth.process(
            self.smooth_lofi_chords,
            self.smooth_lofi_vinyl,
            self.smooth_lofi_wow,
            self.sr,
        );

        let mix_l =
            (delta_l + sch_l + sig_432 + sig_sub + sig_noise + amb_l + lofi_l) * self.smooth_master;
        let mix_r =
            (delta_r + sch_r + sig_432 + sig_sub + sig_noise + amb_r + lofi_r) * self.smooth_master;

        let out_l = mix_l.tanh();
        let out_r = mix_r.tanh();

        if push_to_scope {
            self.scope_decimator = (self.scope_decimator + 1) & 31;
            if self.scope_decimator == 0 {
                model.push_scope(out_l, out_r);
            }
        }

        (out_l, out_r)
    }
}

pub fn start_audio_stream(model: Arc<AudioModel>) -> Result<cpal::Stream, String> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .ok_or_else(|| "Nenhum dispositivo de saída de áudio encontrado".to_string())?;

    let config = device
        .default_output_config()
        .map_err(|e| format!("Erro na configuração de áudio: {e}"))?;

    let sample_rate = config.sample_rate().0 as f32;
    let channels = config.channels() as usize;
    let mut dsp = DspController::new(sample_rate);
    let err_fn = |err| eprintln!("Erro no stream cpal: {err}");

    let stream = match config.sample_format() {
        cpal::SampleFormat::F32 => device
            .build_output_stream(
                &config.into(),
                move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                    for frame in data.chunks_mut(channels) {
                        let (l, r) = dsp.next_stereo_frame(&model, true);
                        if channels == 1 {
                            frame[0] = 0.5 * (l + r);
                        } else {
                            frame[0] = l;
                            frame[1] = r;
                            for ch in &mut frame[2..] {
                                *ch = 0.5 * (l + r);
                            }
                        }
                    }
                },
                err_fn,
                None,
            )
            .map_err(|e| e.to_string())?,
        cpal::SampleFormat::I16 => device
            .build_output_stream(
                &config.into(),
                move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                    for frame in data.chunks_mut(channels) {
                        let (l, r) = dsp.next_stereo_frame(&model, true);
                        let l_i16 = (l * i16::MAX as f32) as i16;
                        let r_i16 = (r * i16::MAX as f32) as i16;
                        if channels == 1 {
                            frame[0] = l_i16.saturating_add(r_i16) / 2;
                        } else {
                            frame[0] = l_i16;
                            frame[1] = r_i16;
                        }
                    }
                },
                err_fn,
                None,
            )
            .map_err(|e| e.to_string())?,
        _ => return Err("Formato de amostra não suportado".to_string()),
    };

    stream.play().map_err(|e| e.to_string())?;
    Ok(stream)
}

pub fn export_wav_session(
    model: &AudioModel,
    duration_secs: u32,
    filepath: &str,
) -> std::io::Result<()> {
    let sr: u32 = 44_100;
    let channels: u16 = 2;
    let total_frames = sr * duration_secs;
    let data_bytes = total_frames * (channels as u32) * 2;
    let riff_size = 36 + data_bytes;

    let file = File::create(filepath)?;
    let mut writer = BufWriter::new(file);

    writer.write_all(b"RIFF")?;
    writer.write_all(&p32(riff_size))?;
    writer.write_all(b"WAVEfmt ")?;
    writer.write_all(&p32(16))?;
    writer.write_all(&p16(1))?;
    writer.write_all(&p16(channels))?;
    writer.write_all(&p32(sr))?;
    writer.write_all(&p32(sr * (channels as u32) * 2))?;
    writer.write_all(&p16(channels * 2))?;
    writer.write_all(&p16(16))?;
    writer.write_all(b"data")?;
    writer.write_all(&p32(data_bytes))?;

    let mut dsp = DspController::new(sr as f32);
    for _ in range(total_frames as i64) {
        let (l, r) = dsp.next_stereo_frame(model, false);
        let l_pcm = (l.clamp(-1.0, 1.0) * 32767.0) as i16;
        let r_pcm = (r.clamp(-1.0, 1.0) * 32767.0) as i16;
        writer.write_all(&p16(l_pcm as u16))?;
        writer.write_all(&p16(r_pcm as u16))?;
    }

    writer.flush()?;
    Ok(())
}
