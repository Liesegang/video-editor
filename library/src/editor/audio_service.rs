use crate::core::audio::engine::AudioEngine;
use crate::core::cache::CacheManager;
use crate::model::project::project::Project;
use std::sync::{Arc, RwLock};

pub struct AudioService {
    project: Arc<RwLock<Project>>,
    audio_engine: Arc<AudioEngine>,
    cache_manager: Arc<CacheManager>,
    next_write_sample: std::sync::atomic::AtomicU64,
}

impl AudioService {
    pub fn new(
        project: Arc<RwLock<Project>>,
        audio_engine: Arc<AudioEngine>,
        cache_manager: Arc<CacheManager>,
    ) -> Self {
        Self {
            project,
            audio_engine,
            cache_manager,
            next_write_sample: std::sync::atomic::AtomicU64::new(0),
        }
    }

    pub fn get_audio_engine(&self) -> Arc<AudioEngine> {
        self.audio_engine.clone()
    }

    pub fn reset_audio_pump(&self, time: f64) {
        self.audio_engine.set_time(time);

        let sample_rate = self.audio_engine.get_sample_rate();
        let channels = self.audio_engine.get_channels();
        let sample_pos = (time * sample_rate as f64).round() as u64;

        // Zapping: Generate 50ms preview
        let preview_duration = 0.05;
        let frames = (preview_duration * sample_rate as f64) as usize;
        let scrub_samples = if let Ok(project) = self.project.read() {
            if let Some(comp) = project.compositions.first() {
                crate::core::audio::mixer::mix_samples(
                    &project.assets,
                    &project,
                    comp,
                    &self.cache_manager,
                    sample_pos,
                    frames,
                    sample_rate,
                    channels as u32,
                )
            } else {
                vec![0.0; frames * channels as usize]
            }
        } else {
            vec![0.0; frames * channels as usize]
        };

        // Push and advance
        self.audio_engine.push_samples(&scrub_samples);
        self.next_write_sample.store(
            sample_pos + frames as u64,
            std::sync::atomic::Ordering::Relaxed,
        );
    }

    pub fn pump_audio(&self) {
        let available = self.audio_engine.available_slots();
        if available == 0 {
            return;
        }

        // Limit chunk size to avoid stalling the UI thread with massive mixing
        let chunk_size = available.min(16384);

        let sample_rate = self.audio_engine.get_sample_rate();
        let channels = self.audio_engine.get_channels();

        let start_sample = self
            .next_write_sample
            .load(std::sync::atomic::Ordering::Relaxed);

        // Safety check
        if chunk_size < (channels as usize) {
            return;
        }

        let frames_to_write = chunk_size / (channels as usize);

        // Use shared mixing logic
        let mix_buffer = if let Ok(project) = self.project.read() {
            if let Some(comp) = project.compositions.first() {
                crate::core::audio::mixer::mix_samples(
                    &project.assets,
                    &project,
                    comp,
                    &self.cache_manager,
                    start_sample,
                    frames_to_write,
                    sample_rate,
                    channels as u32,
                )
            } else {
                vec![0.0; frames_to_write * channels as usize]
            }
        } else {
            vec![0.0; frames_to_write * channels as usize]
        };

        // Push to Engine
        self.audio_engine.push_samples(&mix_buffer);

        // Advance cursor
        self.next_write_sample
            .fetch_add(frames_to_write as u64, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn trigger_audio_loading(&self, asset_id: uuid::Uuid, path: String) {
        let cache_manager = self.cache_manager.clone();
        let target_sample_rate = self.audio_engine.get_sample_rate();

        std::thread::spawn(move || {
            use crate::core::audio::loader::AudioLoader;
            match AudioLoader::load_entire_file(&path, target_sample_rate) {
                Ok(data) => {
                    cache_manager.put_audio(asset_id, data);
                    log::info!("Loaded audio for asset {}", asset_id);
                }
                Err(e) => {
                    log::error!("Failed to load audio for asset {}: {}", asset_id, e);
                }
            }
        });
    }

    pub fn render_audio(&self, start_time: f64, duration: f64) -> Vec<f32> {
        let sample_rate = self.audio_engine.get_sample_rate();
        let channels = self.audio_engine.get_channels();

        // Calculate samples
        let start_sample = (start_time * sample_rate as f64).round() as u64;
        let frames = (duration * sample_rate as f64).round() as usize;

        if let Ok(project) = self.project.read() {
            if let Some(comp) = project.compositions.first() {
                crate::core::audio::mixer::mix_samples(
                    &project.assets,
                    &project,
                    comp,
                    &self.cache_manager,
                    start_sample,
                    frames,
                    sample_rate,
                    channels as u32,
                )
            } else {
                vec![0.0; frames * channels as usize]
            }
        } else {
            vec![0.0; frames * channels as usize]
        }
    }

    // Helper to update reference when project is replaced (New/Load)
    pub fn set_project(&mut self, project: Arc<RwLock<Project>>) {
        self.project = project;
    }
    pub fn get_cache_manager(&self) -> Arc<CacheManager> {
        self.cache_manager.clone() // Arc clone
    }
}
