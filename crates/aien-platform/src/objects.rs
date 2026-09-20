use crate::PlatformError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ObjectId(pub [u8; 32]);

pub struct ObjectRef<'a> {
    pub id: ObjectId,
    pub data: &'a [u8],
}

pub struct ObjectWrite<'a> {
    pub id: Option<ObjectId>,
    pub data: &'a [u8],
    pub tags: &'a [&'a str],
}

pub trait ObjectStore {
    fn read(&self, id: ObjectId) -> Result<ObjectRef<'_>, PlatformError>;
    fn commit(&self, object: ObjectWrite<'_>) -> Result<ObjectId, PlatformError>;
}
