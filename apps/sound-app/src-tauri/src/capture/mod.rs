pub mod fake;

#[derive(Clone, serde::Serialize)]
#[allow(dead_code)] // Unused until Task 3 wires AudioCapture into commands.rs
pub struct AudioSource {
  pub id: String,
  pub name: String,
}

#[derive(Debug, Clone)]
#[allow(dead_code)] // Unused until Task 3 wires AudioCapture into commands.rs
pub struct CaptureError {
  pub message: String,
}

#[allow(dead_code)] // Unused until Task 3 wires AudioCapture into commands.rs
pub type FrameCallback = Box<dyn Fn(Vec<i16>) + Send + 'static>;

#[allow(dead_code)] // Unused until Task 3 wires AudioCapture into commands.rs
pub trait AudioCapture: Send {
  fn list_sources(&self) -> Result<Vec<AudioSource>, CaptureError>;
  fn start(&mut self, source_id: &str, on_frame: FrameCallback) -> Result<(), CaptureError>;
  fn pause(&mut self);
  #[allow(dead_code)] // Unused until Task 3 wires AudioCapture into commands.rs
  fn resume(&mut self);
  fn stop(&mut self) -> Result<(), CaptureError>;
}
