//! The WoodSketch document: geometry, groups and components, selection and undo history.
//! No UI or GPU dependencies.

mod arena;
pub mod board;
pub mod curves;
pub mod cutlist;
pub mod cutplan;
pub mod cutsheet;
mod document;
pub mod export;
pub mod file;
pub mod library;
pub mod part;
mod lines;
mod model;
mod offset;
mod push_pull;
mod scene;
mod transform;
pub mod units;

pub use document::{Document, Selection};
pub use model::{EPS, Edge, EdgeId, Entity, Face, FaceEntry, FaceId, Guide, GuideId, Model, VertexId};
pub use offset::OffsetError;
pub use push_pull::PushPullError;
pub use scene::{
    Container, Definition, DefinitionId, DefinitionKind, Instance, InstanceId, Material, MaterialId, MaterialKind,
    Owner, Scene, Tag, TagId, View,
};
pub use transform::{MoveError, rotation, scaling};
