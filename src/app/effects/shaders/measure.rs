//! What a project shader costs per pixel against a plain sprite, measured the way the engine's
//! effect cost table is: full-screen layers of it drawn `FEW` and `MANY` deep, fastest of
//! `ROUNDS` each, and the slope between them over a plain sprite's slope, so one-off costs
//! cancel. It runs on its own thread with its own renderer, so the editor keeps drawing.

use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::Instant;

use quartz_project::shader_asset::{MeasuredWeight, ProjectShaderAsset};
use quartz_project::shader_engine::ShaderBridge;
use wgpu_canvas::{Area, Effect, EffectColor, HeadlessRenderer, Item, MegaShaderSprite, VfxFlags};

/// A measurement in flight, and its answer.
pub(in crate::app) struct ShaderMeasure {
    running: Option<Running>,
    /// The last measurement that failed, and why.
    pub failed: Option<String>,
}

struct Running {
    shader: String,
    source_hash: u64,
    answer: Receiver<Result<MeasuredWeight, String>>,
}

/// One timed renderer and the two sprites it compares.
struct Bench {
    renderer: HeadlessRenderer,
    plain: MegaShaderSprite,
    shader: MegaShaderSprite,
}

impl ShaderMeasure {
    const WIDTH: u32 = 1356;
    const HEIGHT: u32 = 610;
    const FEW: usize = 12;
    const MANY: usize = 36;
    const ROUNDS: usize = 9;

    pub fn new() -> Self {
        Self { running: None, failed: None }
    }

    /// The shader being measured, if any.
    pub fn measuring(&self) -> Option<&str> {
        self.running.as_ref().map(|running| running.shader.as_str())
    }

    /// Start measuring `asset` unless a measurement is already running.
    pub fn start(&mut self, asset: &ProjectShaderAsset) {
        if self.running.is_some() {
            return;
        }
        let (send, answer) = std::sync::mpsc::channel();
        let shader = asset.clone();
        std::thread::spawn(move || {
            let _ = send.send(Self::measure(&shader));
        });
        self.failed = None;
        self.running = Some(Running { shader: asset.name.clone(), source_hash: asset.source_hash(), answer });
    }

    /// A finished measurement, put on the shader it was for while that shader's source is
    /// still the measured one. True when a shader changed.
    pub fn collect(&mut self, shaders: &mut [ProjectShaderAsset]) -> bool {
        let Some(running) = &self.running else { return false };
        let answer = match running.answer.try_recv() {
            Ok(answer) => answer,
            Err(TryRecvError::Empty) => return false,
            Err(TryRecvError::Disconnected) => Err("the measurement stopped".to_owned()),
        };
        let Some(running) = self.running.take() else { return false };
        let measured = match answer {
            Ok(measured) => measured,
            Err(error) => {
                self.failed = Some(error);
                return false;
            }
        };
        let Some(asset) = shaders.iter_mut().find(|asset| asset.name == running.shader && asset.source_hash() == running.source_hash) else { return false };
        asset.measured = Some(measured);
        true
    }

    fn measure(asset: &ProjectShaderAsset) -> Result<MeasuredWeight, String> {
        let renderer = pollster::block_on(HeadlessRenderer::new(Self::WIDTH, Self::HEIGHT))?;
        let color = EffectColor::srgb8(255, 160, 60);
        let shader = ShaderBridge::effect(&asset.choice(), Some(ShaderBridge::source(asset))).sprite(color, 1.0);
        let plain = Effect::Animated { flags: VfxFlags::NONE, alpha: 0.8 }.sprite(color);
        let mut bench = Bench { renderer, plain, shader };
        let plain_ms = bench.per_screen(false);
        if let Some((_, problem)) = bench.renderer.project_shader_problems().into_iter().next() {
            return Err(problem.message);
        }
        let shader_ms = bench.per_screen(true);
        if let Some((_, problem)) = bench.renderer.project_shader_problems().into_iter().next() {
            return Err(problem.message);
        }
        let measured_on = format!(
            "{}, {}x{}, slope between {} and {} full-screen layers, fastest of {}",
            bench.renderer.adapter_name(),
            Self::WIDTH,
            Self::HEIGHT,
            Self::FEW,
            Self::MANY,
            Self::ROUNDS
        );
        let weight = ((shader_ms / plain_ms).max(1.0) * 100.0).round() / 100.0;
        Ok(MeasuredWeight { weight: weight as f32, measured_on, source_hash: asset.source_hash() })
    }
}

impl Bench {
    /// Milliseconds per full screen: the slope between `FEW` and `MANY` layers.
    fn per_screen(&mut self, project: bool) -> f64 {
        let few = self.fastest(project, ShaderMeasure::FEW);
        let many = self.fastest(project, ShaderMeasure::MANY);
        ((many - few) / (ShaderMeasure::MANY - ShaderMeasure::FEW) as f64).max(1e-4)
    }

    /// The fastest of `ROUNDS` renders, after one that builds the pipeline.
    fn fastest(&mut self, project: bool, layers: usize) -> f64 {
        self.renderer.render(self.frame(project, layers));
        (0..ShaderMeasure::ROUNDS)
            .map(|_| {
                let frame = self.frame(project, layers);
                let started = Instant::now();
                self.renderer.render(frame);
                started.elapsed().as_secs_f64() * 1000.0
            })
            .fold(f64::MAX, f64::min)
    }

    fn frame(&self, project: bool, layers: usize) -> Vec<(Area, Item)> {
        let mut sprite = if project { self.shader.clone() } else { self.plain.clone() };
        sprite.instance.world_position = (0.5, 0.5);
        sprite.instance.scale = (1.0, 1.0);
        (0..layers).map(|_| (Area { offset: (0.0, 0.0), bounds: None }, Item::MegaShaderSprite(sprite.clone()))).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::ShaderMeasure;
    use quartz_project::shader_asset::ProjectShaderAsset;

    #[test]
    fn a_project_shader_is_measured_against_a_plain_sprite_for_its_current_source() {
        let asset = ProjectShaderAsset::layered("measured");
        let measured = match ShaderMeasure::measure(&asset) {
            Ok(measured) => measured,
            Err(error) if error.contains("adapter") || error.contains("Adapter") => return,
            Err(error) => panic!("{error}"),
        };
        assert!(measured.weight >= 1.0, "{measured:?}");
        assert_eq!(measured.source_hash, asset.source_hash());
        let mut shaders = vec![asset];
        let mut measure = ShaderMeasure::new();
        measure.start(&shaders[0]);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
        while !measure.collect(&mut shaders) && measure.failed.is_none() && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(shaders[0].current_weight().is_some(), "{:?}", measure.failed);
    }
}
