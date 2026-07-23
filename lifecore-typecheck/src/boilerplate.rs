use lifecore::LifeCore;

use crate::{
    FieldDecl, FieldStatus, Type, TypeCheckFactory, TypeCheckMessage, TypeCheckOutput, ValueType,
};

impl Type {
    pub fn new(
        name: &str,
        fields: &[(&str, (ValueType, FieldStatus))],
        outgoing: &[&str],
        incoming: &[&str],
        additional_override: &[&str],
    ) -> Self {
        Self {
            name: name.to_string(),
            outgoing: outgoing.iter().map(|x| x.to_string()).collect(),
            incoming: incoming.iter().map(|x| x.to_string()).collect(),
            additional_override: additional_override.iter().map(|x| x.to_string()).collect(),
            body: fields
                .iter()
                .map(|(name, value)| FieldDecl {
                    name: name.to_string(),
                    ty: value.0.clone(),
                    status: value.1,
                })
                .collect(),
        }
    }
}

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
