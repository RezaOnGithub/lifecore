use lifecore::{EventContext, LifeCore, PointedEventContext};

pub fn pointed_index(context: PointedEventContext) -> usize {
    serde_json::to_value(context)
        .expect("PointedEventContext must serialize")
        .get("thing")
        .and_then(serde_json::Value::as_u64)
        .expect("PointedEventContext serialization must contain thing") as usize
}

pub fn empty_context() -> EventContext {
    PointedEventContext::new(0).shrink_to_exclusive_left()
}

pub fn latest_context<LC: LifeCore>(lifecore: &LC) -> EventContext {
    lifecore
        .last()
        .map_or_else(empty_context, PointedEventContext::window)
}

pub fn bounded_context(context: PointedEventContext) -> PointedEventContext {
    PointedEventContext::new(pointed_index(context))
}

pub fn same_event(left: PointedEventContext, right: PointedEventContext) -> bool {
    left.absolute() == right.absolute()
}
