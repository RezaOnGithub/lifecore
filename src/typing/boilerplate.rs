use super::{FieldDecl, FieldStatus, Type, ValueType};

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
            body: fields
                .iter()
                .map(|(name, (ty, status))| FieldDecl::new(*name, ty.clone(), *status))
                .collect(),
            outgoing: outgoing.iter().map(|value| value.to_string()).collect(),
            incoming: incoming.iter().map(|value| value.to_string()).collect(),
            additional_override: additional_override
                .iter()
                .map(|value| value.to_string())
                .collect(),
        }
    }
}
