pub mod fake;
pub mod linux_pulse;

#[derive(Clone, serde::Serialize)]
pub struct AudioSource {
  pub id: String,
  pub name: String,
}

#[derive(Debug, Clone)]
pub struct CaptureError {
  pub message: String,
}

#[derive(Debug, Clone, Copy)]
pub struct AudioFormat {
  pub sample_rate: u32,
  pub channels: u8,
}

pub type FrameCallback = Box<dyn Fn(Result<Vec<i16>, CaptureError>) + Send + 'static>;

pub trait AudioCapture: Send {
  fn list_sources(&self) -> Result<Vec<AudioSource>, CaptureError>;
  fn start(&mut self, source_id: &str, on_frame: FrameCallback) -> Result<(), CaptureError>;
  fn format(&self) -> AudioFormat;
  fn pause(&mut self);
  fn resume(&mut self);
  fn stop(&mut self) -> Result<(), CaptureError>;
}
