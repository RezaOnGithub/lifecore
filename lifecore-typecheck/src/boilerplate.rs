use lifecore::LifeCore;

use crate::{TypeCheckFactory, TypeCheckMessage, TypeCheckOutput};

impl TypeCheckOutput {
    pub fn push(&mut self, m: TypeCheckMessage) {
        self.0.push(m);
    }
}

impl<LC: LifeCore> Default for TypeCheckFactory<LC> {
    fn default() -> Self {
        Self {
            _x: Default::default(),
        }
    }
}
