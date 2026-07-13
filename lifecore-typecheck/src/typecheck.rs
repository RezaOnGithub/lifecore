use std::collections::HashMap;

use itertools::Itertools;
use lifecore::{
    AnalysisReport, ConstraintSystemMessage, EventContext, LifeCore, NodeType, NodeTypeFieldStatus,
    SuggestedAction, TypedBody,
};

use crate::{TypeCheck, TypeCheckOutput, TypeCheckReport};

impl TypeCheck {
    pub fn typecheck_initialize<LC: LifeCore>(&mut self, lc: &LC) -> TypeCheckOutput {
        if lc
            .types(EventContext::universe())
            .any(|(name, _)| name == NodeType::name())
        {
            return TypeCheckOutput(vec![]);
        }

        TypeCheckOutput(vec![TypeCheckReport::AddBootstrapNode])
    }

    pub fn typecheck_analyze<LC: LifeCore>(&self, lc: &LC, extent: EventContext) -> TypeCheckOutput {
        let mut r = TypeCheckOutput(vec![]);
        use TypeCheckReport::*;
        // check whether a Node_Type declaring some type X is not clashing with an older but non-stale
        // Node_Type declaring a type of the same name.
        for some_type_at_some_point_node in lc
            .nodes(extent)
            .filter(|node| node.ty_name.as_str() == NodeType::name())
        {
            let type_table = lc
                .types(some_type_at_some_point_node.e.exclusive_left_context())
                .collect::<HashMap<_, _>>();

            let Ok(some_type_at_some_point) =
                NodeType::try_from(&some_type_at_some_point_node.field)
            else {
                r.push(MalformedNodeTypeBody {
                    offending_event: some_type_at_some_point_node.e,
                });
                continue;
            };
            if let Some((older_type_index, _older_type)) =
                type_table.get(some_type_at_some_point.name.as_str())
            {
                // The only scenario this is valid is if the new node override the old one
                if some_type_at_some_point_node
                    .parent
                    .is_none_or(|x| x.absolute() != older_type_index.absolute())
                {
                    r.push(DuplicateTypeDefinition {
                        name: some_type_at_some_point.name.to_string(),
                        offending_event: some_type_at_some_point_node.e.inclusive_left_context(),
                    });
                }
            }
        }

        // Check overrides in types
        for last in lc.last().unwrap().inclusive_left() {
            let ctx = last.inclusive_left_context();
            let type_table = lc.types(ctx).collect::<HashMap<_, _>>();
            // Check that override types exist
            // TODO I'm not sure whether this should be an error or not.
            // What it's causing at the moment is that interrelated Node_Types have to complete
            // a ritual to allow overriding each othe. I'm demoting this to a warning for now.
            for (ty_event, ty) in type_table.values() {
                for override_ty in ty.additional_override.iter() {
                    if !type_table.contains_key(override_ty) {
                        r.push(NonexistantOverrideType {
                            offending_event: *ty_event,
                            base_ty_name: ty.name.to_string(),
                            override_ty_name: override_ty.to_string(),
                            relevance: vec![*ty_event],
                        })
                    }
                }
            }
        }

        // Check nodes for their fields
        for last_event_in_context in lc.window(EventContext::universe().items()) {
            let ctx = last_event_in_context
                .ctx()
                .absolute()
                .inclusive_left_context();
            for pointed_context in ctx
                .items()
                .filter(|e| !lc.is_stale(*e) && lc.get(*e).is_some_and(|x| x.is_node()))
            {
                let node = lc.node(pointed_context);
                if node.ty.is_none() {
                    r.push(NodeTypeNonexistant {
                        offending_event: pointed_context,
                        ty_name: node.ty_name.to_string(),
                    });
                    continue;
                }
                let node_type = node.ty.unwrap();

                // Fields that may be present and mandatory
                for (field_name, (field_type, field_policy)) in node_type.field.iter() {
                    match field_policy {
                    NodeTypeFieldStatus::Mandatory => {
                        if !node.field.0.contains_key(field_name) {
                            r.push(ElidedMandatoryField {
                                offending_event: pointed_context,
                                ty_name: node_type.name.to_string(),
                                field_name: field_name.to_string(),
                            });
                        }
                    }
                    NodeTypeFieldStatus::Encouraged => {
                        if !node.field.0.contains_key(field_name) {
                            r.push(ElidedEncouragedField {
                                offending_event: pointed_context,
                                ty_name: node_type.name.to_string(),
                                field_name: field_name.to_string(),
                            });
                        }
                    }
                    NodeTypeFieldStatus::Deprecated => {
                        if node.field.0.contains_key(field_name) {
                            r.push(DeprecatedField {
                                offending_event: pointed_context,
                                ty_name: node_type.name.to_string(),
                                field_name: field_name.to_string(),
                            });
                        }
                    }
                    NodeTypeFieldStatus::Optional => {
                        // Nothing to do.
                    }
                    }
                    if let Some(field_payload) = node.field.0.get(field_name)
                        && field_payload.discriminant() != *field_type
                    {
                        r.push(FieldTypeMismatch {
                            offending_event: pointed_context,
                            ty_name: node_type.name.to_string(),
                            field_name: field_name.to_string(),
                            expected_type: *field_type,
                            present_type: field_payload.discriminant(),
                        })
                    }
                }

                // Extraneous fields
                for extra_field in node.field.0.keys().filter(|body_field| {
                    !node_type
                        .field
                        .iter()
                        .map(|x| x.0.as_str())
                        .contains(body_field.as_str())
                }) {
                    r.push(ExtraneousField {
                        offending_event: pointed_context,
                        ty_name: node_type.name.to_string(),
                        field_name: extra_field.to_string(),
                    })
                }
            }
        }

        // Check edges ends so that they allow for that edge
        for last_event_in_context in lc.window(EventContext::universe().items()) {
            let ctx = last_event_in_context
                .ctx()
                .absolute()
                .inclusive_left_context();
            let type_table = lc.types(ctx).collect::<HashMap<_, _>>();

            for pointed_context in ctx
                .items()
                .filter(|e| !lc.is_stale(*e) && lc.get(*e).is_some_and(|x| x.is_edge()))
            {
                // from end -> "outgoing" relative to that node
                // to end -> "incoming" relative to that node
                let edge = lc.edge(pointed_context);
                let outgoing_node = lc.node(edge.from);
                let incoming_node = lc.node(edge.to);

                if !(type_table.contains_key(outgoing_node.ty_name.as_str())
                    && type_table.contains_key(incoming_node.ty_name.as_str()))
                {
                    // We have bigger problems
                    assert!(r.0.iter().any(|x| {
                        match x {
                        NodeTypeNonexistant { ty_name, .. } => [
                            outgoing_node.ty_name.as_str(),
                            incoming_node.ty_name.as_str(),
                        ]
                        .contains(&ty_name.as_str()),
                        _ => false,
                        }
                    }));
                    continue;
                }

                if !outgoing_node
                    .ty
                    .unwrap()
                    .outgoing
                    .iter()
                    .map(String::as_str)
                    .contains(edge.ty_name.as_str())
                {
                    err!(
                        pointed_context,
                        "Edge head node of type {} does not allow outgoing edge of type {}",
                        outgoing_node.ty_name.as_str(),
                        edge.ty_name.as_str()
                    );
                }
                // COPY_PASTE
                if !incoming_node
                    .ty
                    .unwrap()
                    .incoming
                    .iter()
                    .map(String::as_str)
                    .contains(edge.ty_name.as_str())
                {
                    err!(
                        pointed_context,
                        "Edge tail node of type {} does not allow incoming edge of type {}",
                        incoming_node.ty_name.as_str(),
                        edge.ty_name.as_str()
                    );
                }
            }
        }

        // Check whether a child is allowed to override parent at the point of override
        for last_event_in_context in lc.window(EventContext::universe().items()) {
            let ctx = last_event_in_context
                .ctx()
                .absolute()
                .exclusive_left_context();
            if last_event_in_context.is_edge()
                || last_event_in_context.is_node_and(|x| x.parent.is_none())
            {
                continue;
            }

            let child_type_name = last_event_in_context.as_node().ty_name.clone();
            let parent_in_context = ctx.look_at(last_event_in_context.as_node().parent.unwrap());
            let Some(parent_type) = lc.node(parent_in_context).ty else {
                // We have bigger problems.
                continue;
            };

            if !parent_type.additional_override.contains(&child_type_name) {
                err!(
                    last_event_in_context.ctx().inclusive_left_context(),
                    "The parent type {} does not allow a node of type {} to override it",
                    parent_type.name.as_str(),
                    child_type_name.as_str()
                );
            }
        }

        // Check Node Classes
        for last_event_in_context in lc.window(EventContext::universe().items()) {
            let ctx = last_event_in_context
                .ctx()
                // .absolute()
                .inclusive_left_context();
            // I expect `.absolute` to not make any difference but let's defensively check for it for now. TODO remove
            assert!(
                last_event_in_context
                    .ctx()
                    .absolute()
                    .inclusive_left_context()
                    == ctx
            );
            for fresh_node in lc.nodes(ctx).filter(|x| !lc.is_stale(x.e)) {
                let Some(ty) = fresh_node.ty else {
                    // We have bigger problems
                    continue;
                };
                let outgoing_count = lc.fresh_outgoing_edges_of(fresh_node.e).count();
                let incoming_count = lc.fresh_incoming_edges_to(fresh_node.e).count();
                match ty.class {
                lifecore::NodeTypeClass::Spike => {
                    if incoming_count > 0 {
                        err!(fresh_node.e, "Does not respect node class");
                    }
                }
                lifecore::NodeTypeClass::Hub => {
                    if outgoing_count > 0 {
                        err!(fresh_node.e, "Does not respect node class");
                    }
                }
                lifecore::NodeTypeClass::Toothpick => {
                    if outgoing_count != 1 || incoming_count > 0 {
                        err!(fresh_node.e, "Does not respect node class");
                    }
                }
                lifecore::NodeTypeClass::EdgeNode => {
                    if outgoing_count != 1 && incoming_count != 1 {
                        err!(fresh_node.e, "Does not respect node class");
                    }
                }
                lifecore::NodeTypeClass::Regular => continue, // Anything goes
                }
            }
        }
        r
    }
}
