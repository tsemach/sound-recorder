pub mod fake;

#[derive(Clone, serde::Serialize)]
pub struct AudioSource {
  pub id: String,
  pub name: String,
}

#[derive(Debug, Clone)]
pub struct CaptureError {
  pub message: String,
}

pub type FrameCallback = Box<dyn Fn(Vec<i16>) + Send + 'static>;

pub trait AudioCapture: Send {
  fn list_sources(&self) -> Result<Vec<AudioSource>, CaptureError>;
  fn start(&mut self, source_id: &str, on_frame: FrameCallback) -> Result<(), CaptureError>;
  fn pause(&mut self);
  fn resume(&mut self);
  fn stop(&mut self) -> Result<(), CaptureError>;
}
