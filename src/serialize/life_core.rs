use crate::{EntryContext, LifeCore, LifeCoreError, Node};

use super::NodeBody;

/// Typed insertion conveniences shared by every [`LifeCore`] implementation.
pub trait LifeCoreSerializeExt: LifeCore {
    fn add_typed_mock<T: NodeBody>(
        &mut self,
        value: T,
        parent: Option<EntryContext>,
    ) -> Result<Node, LifeCoreError> {
        self.add_mock(T::TYPE_NAME, value.into_body(), parent)
    }
}

impl<LC: LifeCore> LifeCoreSerializeExt for LC {}
