//! The SketchCraft document: geometry, selection and undo history. No UI or GPU dependencies.

mod document;
mod model;
mod push_pull;
mod transform;
pub mod units;

pub use document::{Document, Selection};
pub use model::{EPS, Edge, EdgeId, Entity, Face, FaceId, Model, VertexId};
pub use push_pull::PushPullError;
pub use transform::{MoveError, rotation};
