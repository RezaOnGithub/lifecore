use std::{ops::Index, str::FromStr as _};

use itertools::Itertools;

use crate::{
    Circumstance, EntryIndex, LifeCore, LifeCoreList, Payload,
    reference_impl::{AggregateIndex, CircumstanceIndex, StringIndex, Value, ValueIndex, XEntry},
};

impl From<&StringIndex> for usize {
    fn from(value: &StringIndex) -> Self {
        value.0
    }
}

impl From<StringIndex> for usize {
    fn from(value: StringIndex) -> Self {
        (&value).into()
    }
}

impl From<&ValueIndex> for usize {
    fn from(value: &ValueIndex) -> Self {
        value.0
    }
}

impl From<ValueIndex> for usize {
    fn from(value: ValueIndex) -> Self {
        (&value).into()
    }
}

impl From<&AggregateIndex> for usize {
    fn from(value: &AggregateIndex) -> Self {
        value.0
    }
}

impl From<AggregateIndex> for usize {
    fn from(value: AggregateIndex) -> Self {
        (&value).into()
    }
}
impl From<&CircumstanceIndex> for usize {
    fn from(value: &CircumstanceIndex) -> Self {
        value.0
    }
}

impl From<CircumstanceIndex> for usize {
    fn from(value: CircumstanceIndex) -> Self {
        (&value).into()
    }
}

impl Index<StringIndex> for LifeCoreList {
    type Output = str;

    fn index(&self, index: StringIndex) -> &Self::Output {
        self.string_table[index.0].as_str()
    }
}

impl Index<CircumstanceIndex> for LifeCoreList {
    type Output = Circumstance;

    fn index(&self, index: CircumstanceIndex) -> &Self::Output {
        &self.circumstance_table[index.0]
    }
}

impl Index<AggregateIndex> for LifeCoreList {
    type Output = Vec<ValueIndex>;

    fn index(&self, index: AggregateIndex) -> &Self::Output {
        &self.aggregate_table[index.0]
    }
}

impl Index<ValueIndex> for LifeCoreList {
    type Output = Value;

    fn index(&self, index: ValueIndex) -> &Self::Output {
        &self.value_table[index.0]
    }
}

impl LifeCoreList {

    pub(in crate::reference_impl) fn raw(&self, x: EntryIndex) -> XEntry {
        self.entry[x.0].clone()
    }

    pub(in crate::reference_impl) fn get_or_insert_value(&mut self, v: &Value) -> ValueIndex {
        if let Some(i) = self.value_table.get_index_of(v).map(|x| ValueIndex(x)) {
            i
        } else {
            assert!(
                self.value_table.insert(v.clone()),
                "The value should not be present; we just checked!"
            );
            ValueIndex(self.value_table.len() - 1)
        }
    }

    pub(in crate::reference_impl) fn resolve_value(&self, i: ValueIndex) -> Payload {
        match self.value_table[i.0] {
        Value::PreciseTime(timestamp) => Payload::PreciseTime(timestamp),
        Value::Text(string_index) => Payload::Text(self.resolve_string(string_index)),
        Value::Number(x) => Payload::Number(x),
        Value::Aggregate(aggregate_index) => {
            Payload::Aggregate(self.resolve_aggregate(aggregate_index))
        }
        Value::Serialized(string_index) => {
            Payload::Serialized(crate::JSON::from_str(&self.resolve_string(string_index)).unwrap())
        }
        }
    }

    // COPY_PASTE
    pub(in crate::reference_impl) fn get_or_insert_string(&mut self, v: &str) -> StringIndex {
        if let Some(i) = self.string_table.get_index_of(v).map(|x| StringIndex(x)) {
            i
        } else {
            assert!(
                self.string_table.insert(v.to_string()),
                "The value should not be present; we just checked!"
            );
            StringIndex(self.string_table.len() - 1)
        }
    }

    pub(in crate::reference_impl) fn resolve_string(&self, i: StringIndex) -> String {
        self.string_table[i.0].to_string()
    }

    // COPY_PASTE
    pub(in crate::reference_impl) fn get_or_insert_aggregate(
        &mut self,
        v: &Vec<ValueIndex>,
    ) -> AggregateIndex {
        if let Some(i) = self
            .aggregate_table
            .get_index_of(v)
            .map(|x| AggregateIndex(x))
        {
            i
        } else {
            assert!(
                self.aggregate_table.insert(v.clone()),
                "The value should not be present; we just checked!"
            );
            AggregateIndex(self.aggregate_table.len() - 1)
        }
    }

    pub(in crate::reference_impl) fn resolve_aggregate(&self, i: AggregateIndex) -> Vec<Payload> {
        self.aggregate_table[i.0]
            .iter()
            .map(|x| self.resolve_value(*x))
            .collect()
    }

    // COPY_PASTE
    pub(in crate::reference_impl) fn get_or_insert_circumstance(
        &mut self,
        c: &Circumstance,
    ) -> CircumstanceIndex {
        if let Some(i) = self
            .circumstance_table
            .get_index_of(c)
            .map(|x| CircumstanceIndex(x))
        {
            i
        } else {
            assert!(
                self.circumstance_table.insert(c.clone()),
                "The value should not be present; we just checked!"
            );
            CircumstanceIndex(self.circumstance_table.len() - 1)
        }
    }

    pub(in crate::reference_impl) fn resolve_circumstance(
        &self,
        i: CircumstanceIndex,
    ) -> Circumstance {
        self.circumstance_table[i.0].clone()
    }

    pub(in crate::reference_impl) fn insert_payload(&mut self, v: &Payload) -> ValueIndex {
        match v {
        Payload::PreciseTime(timestamp) => {
            self.get_or_insert_value(&Value::PreciseTime(*timestamp))
        }
        Payload::Text(s) => {
            let string_index = self.get_or_insert_string(s);
            self.get_or_insert_value(&Value::Text(string_index))
        }
        Payload::Number(n) => self.get_or_insert_value(&Value::Number(*n)),
        Payload::Aggregate(payloads) => {
            let value_indices = payloads
                .iter()
                .map(|x| self.insert_payload(x))
                .collect_vec();
            let aggregate_index = self.get_or_insert_aggregate(&value_indices);
            self.get_or_insert_value(&Value::Aggregate(aggregate_index))
        }
        Payload::Serialized(v) => {
            let json_string = v.to_string();
            let string_index = self.get_or_insert_string(&json_string);
            self.get_or_insert_value(&Value::Text(string_index))
        }
        }
    }

    pub(in crate::reference_impl) fn payload_exists(&self, v: &Payload) -> bool {
        // Contrary to what `rust-analyzer` says, `mut` is very much necessary here
        // TODO open an issue?
        let mut clone = self.clone();
        let value_index = clone.insert_payload(v);
        self.value_table.len() > value_index.0
    }

    pub(in crate::reference_impl) fn get_payload(&self, i: &ValueIndex) -> Payload {
        match self[*i] {
        Value::PreciseTime(timestamp) => Payload::PreciseTime(timestamp),
        Value::Text(string_index) => Payload::Text(self[string_index].to_string()),
        Value::Number(i) => Payload::Number(i),
        Value::Aggregate(aggregate_index) => Payload::Aggregate(
            self[aggregate_index]
                .iter()
                .map(|value_index| self.get_payload(value_index))
                .collect(),
        ),
        Value::Serialized(string_index) => Payload::Text(self[string_index].to_string()),
        }
    }
}
