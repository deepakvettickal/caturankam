//! Move, capture, check and game start/end sounds.
//!
//! They're synthesised here (nothing to download or license) and played with
//! macOS's NSSound, which keeps no audio stream open between sounds, so
//! silence costs no CPU. Each sound is built the first time it plays.

/// Samples per second of the synthesised sounds.
const RATE: u32 = 44_100;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Sound {
    Move,
    Capture,
    Check,
    Start,
    End,
}

impl Sound {
    /// The sound for a move: check outranks capture outranks a plain move.
    pub fn for_move(ev: &caturankam_core::MoveEvent) -> Self {
        match (ev.check, ev.capture) {
            (true, _) => Sound::Check,
            (false, true) => Sound::Capture,
            _ => Sound::Move,
        }
    }

    /// Mono 16-bit samples.
    fn samples(self) -> Vec<i16> {
        // (frequency Hz, decay seconds, start s, length s, loudness, noise)
        let tones: &[(f32, f32, f32, f32, f32, f32)] = match self {
            // A short woody tock.
            Sound::Move => &[(520.0, 0.018, 0.0, 0.07, 0.45, 0.25)],
            // Lower, longer and with more knock.
            Sound::Capture => &[(330.0, 0.030, 0.0, 0.11, 0.6, 0.45)],
            // A small bell: a fifth above with a slow decay.
            Sound::Check => &[
                (880.0, 0.12, 0.0, 0.3, 0.35, 0.0),
                (1320.0, 0.09, 0.0, 0.3, 0.2, 0.0),
            ],
            Sound::Start => &[
                (660.0, 0.06, 0.0, 0.12, 0.3, 0.0),
                (880.0, 0.08, 0.1, 0.2, 0.3, 0.0),
            ],
            Sound::End => &[
                (880.0, 0.06, 0.0, 0.12, 0.3, 0.0),
                (660.0, 0.10, 0.1, 0.25, 0.3, 0.0),
            ],
        };
        let total = tones.iter().map(|t| t.2 + t.3).fold(0.0, f32::max);
        let mut out = vec![0f32; (total * RATE as f32) as usize];
        let mut noise = 0x2545_F491u32; // deterministic white noise
        for &(freq, decay, start, len, loud, knock) in tones {
            let first = (start * RATE as f32) as usize;
            let count = (len * RATE as f32) as usize;
            for i in 0..count {
                let t = i as f32 / RATE as f32;
                noise ^= noise << 13;
                noise ^= noise >> 17;
                noise ^= noise << 5;
                let white = (noise as f32 / u32::MAX as f32) * 2.0 - 1.0;
                // The knock is a burst of noise in the first few milliseconds.
                let attack = (t / 0.002).min(1.0);
                let body = (std::f32::consts::TAU * freq * t).sin() * (-t / decay).exp();
                let click = white * knock * (-t / 0.004).exp();
                out[first + i] += loud * attack * (body + click);
            }
        }
        out.iter()
            .map(|s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
            .collect()
    }
}

/// A WAV file (RIFF, PCM, mono, 16-bit) around `samples`.
fn wav(samples: &[i16]) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut w = Vec::with_capacity(44 + data_len as usize);
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&(36 + data_len).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    w.extend_from_slice(&1u16.to_le_bytes()); // PCM
    w.extend_from_slice(&1u16.to_le_bytes()); // mono
    w.extend_from_slice(&RATE.to_le_bytes());
    w.extend_from_slice(&(RATE * 2).to_le_bytes()); // bytes per second
    w.extend_from_slice(&2u16.to_le_bytes()); // block align
    w.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    w.extend_from_slice(b"data");
    w.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        w.extend_from_slice(&s.to_le_bytes());
    }
    w
}

#[cfg(target_os = "macos")]
#[derive(Default)]
pub struct Player {
    cache: std::collections::HashMap<Sound, objc2::rc::Retained<objc2_app_kit::NSSound>>,
}

#[cfg(target_os = "macos")]
impl Player {
    /// Plays `sound`, building it on first use. Must run on the main thread.
    pub fn play(&mut self, sound: Sound) {
        use objc2::AllocAnyThread;
        use objc2_app_kit::NSSound;
        let entry = self.cache.entry(sound);
        let player = match entry {
            std::collections::hash_map::Entry::Occupied(e) => e.into_mut(),
            std::collections::hash_map::Entry::Vacant(e) => {
                let data = objc2_foundation::NSData::from_vec(wav(&sound.samples()));
                let Some(s) = NSSound::initWithData(NSSound::alloc(), &data) else {
                    return; // can't happen for a valid WAV; stay silent if it does
                };
                e.insert(s)
            }
        };
        // Restart rather than ignore a sound that's still playing (fast games).
        player.stop();
        player.play();
    }
}

/// Elsewhere, silence for now.
// shortcut: no audio off macOS; add a backend with the Windows build.
#[cfg(not(target_os = "macos"))]
#[derive(Default)]
pub struct Player;

#[cfg(not(target_os = "macos"))]
impl Player {
    pub fn play(&mut self, _: Sound) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sounds_are_short_and_never_clip_silently() {
        for s in [
            Sound::Move,
            Sound::Capture,
            Sound::Check,
            Sound::Start,
            Sound::End,
        ] {
            let samples = s.samples();
            let secs = samples.len() as f32 / RATE as f32;
            assert!((0.05..=0.4).contains(&secs), "{s:?} lasts {secs}s");
            let peak = samples.iter().map(|x| x.unsigned_abs()).max().unwrap();
            assert!(peak > 3000, "{s:?} is too quiet");
            assert!(peak < i16::MAX as u16, "{s:?} clips");
        }
    }

    #[test]
    fn wav_header_is_valid() {
        let w = wav(&[0, 1, -1]);
        assert_eq!(&w[..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(w[4..8].try_into().unwrap()), 36 + 6);
        assert_eq!(&w[8..16], b"WAVEfmt ");
        assert_eq!(u32::from_le_bytes(w[24..28].try_into().unwrap()), RATE);
        assert_eq!(&w[36..40], b"data");
        assert_eq!(w.len(), 44 + 6);
    }

    #[test]
    fn check_outranks_capture() {
        use caturankam_core::{Color, MoveEvent, Piece, Role, Square};
        let ev = |capture, check| MoveEvent {
            from: Square::E2,
            to: Square::E4,
            piece: Piece {
                color: Color::White,
                role: Role::Pawn,
            },
            capture,
            check,
        };
        assert_eq!(Sound::for_move(&ev(true, true)), Sound::Check);
        assert_eq!(Sound::for_move(&ev(true, false)), Sound::Capture);
        assert_eq!(Sound::for_move(&ev(false, false)), Sound::Move);
    }
}
