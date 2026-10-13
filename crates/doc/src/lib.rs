//! The SketchCraft document: geometry, groups and components, selection and undo history.
//! No UI or GPU dependencies.

mod arena;
mod document;
mod model;
mod push_pull;
mod scene;
mod transform;
pub mod units;

pub use document::{Document, Selection};
pub use model::{EPS, Edge, EdgeId, Entity, Face, FaceId, Model, VertexId};
pub use push_pull::PushPullError;
pub use scene::{Container, Definition, DefinitionId, DefinitionKind, Instance, InstanceId, Owner, Scene, View};
pub use transform::{MoveError, rotation};
