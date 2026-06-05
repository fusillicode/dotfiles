use serde::Serialize;

#[derive(rkyv::Archive, Clone, Copy, Debug, rkyv::Deserialize, Eq, PartialEq, Serialize, rkyv::Serialize)]
pub enum PaneScrollDirection {
    Down,
    Up,
}
