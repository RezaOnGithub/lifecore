use std::collections::BTreeSet;

use lifecore::{
    Entry, EntryIndex, LifeCore, LifeCoreList, OrderedSet,
    serialize::NodeBody,
    typing::{FieldDecl, Type},
};

use crate::{
    app::TypeRecord,
    cli::{TypeDeclarationArgs, TypePatchArgs},
    error::AppError,
    locator::{EntryExpectation, ResolvedEntry, resolve_index},
    state::CliStateV1,
    transaction::PlannedChange,
    view::GraphView,
};

pub struct TypeCreateRequest {
    name: String,
    fields: Vec<FieldDecl>,
    outgoing: Vec<String>,
    incoming: Vec<String>,
    allows_override: Vec<String>,
}

impl TypeCreateRequest {
    pub fn parse(name: String, args: &TypeDeclarationArgs) -> Result<Self, AppError> {
        Ok(Self {
            name,
            fields: parse_field_declarations(&args.fields, "type declaration", false)?,
            outgoing: args.outgoing.clone(),
            incoming: args.incoming.clone(),
            allows_override: args.allows_override.clone(),
        })
    }

    pub fn build(self) -> Type {
        Type {
            name: self.name,
            body: self.fields.into_iter().collect(),
            outgoing: self.outgoing.into_iter().collect(),
            incoming: self.incoming.into_iter().collect(),
            additional_override: self.allows_override.into_iter().collect(),
        }
    }
}

pub struct TypePatch {
    new_name: Option<String>,
    fields: Vec<FieldDecl>,
    drop_fields: BTreeSet<String>,
    allow_outgoing: Vec<String>,
    disallow_outgoing: BTreeSet<String>,
    allow_incoming: Vec<String>,
    disallow_incoming: BTreeSet<String>,
    allows_override: Vec<String>,
    disallow_override: BTreeSet<String>,
}

impl TypePatch {
    pub fn parse(new_name: Option<String>, args: &TypePatchArgs) -> Result<Self, AppError> {
        let fields = parse_field_declarations(&args.fields, "type patch", true)?;
        let drop_fields = args
            .drop_fields
            .iter()
            .map(|name| parse_field_name(name))
            .collect::<Result<BTreeSet<_>, _>>()?;

        ensure_no_field_conflicts(&fields, &drop_fields)?;
        ensure_no_permission_conflicts(
            "outgoing edge permission",
            &args.allow_outgoing,
            &args.disallow_outgoing,
        )?;
        ensure_no_permission_conflicts(
            "incoming edge permission",
            &args.allow_incoming,
            &args.disallow_incoming,
        )?;
        ensure_no_permission_conflicts(
            "override permission",
            &args.allows_override,
            &args.disallow_override,
        )?;

        Ok(Self {
            new_name,
            fields,
            drop_fields,
            allow_outgoing: args.allow_outgoing.clone(),
            disallow_outgoing: args.disallow_outgoing.iter().cloned().collect(),
            allow_incoming: args.allow_incoming.clone(),
            disallow_incoming: args.disallow_incoming.iter().cloned().collect(),
            allows_override: args.allows_override.clone(),
            disallow_override: args.disallow_override.iter().cloned().collect(),
        })
    }

    pub fn validate(&self) -> Result<(), AppError> {
        ensure_no_field_conflicts(&self.fields, &self.drop_fields)?;
        ensure_no_permission_conflicts(
            "outgoing edge permission",
            &self.allow_outgoing,
            &self.disallow_outgoing.iter().cloned().collect::<Vec<_>>(),
        )?;
        ensure_no_permission_conflicts(
            "incoming edge permission",
            &self.allow_incoming,
            &self.disallow_incoming.iter().cloned().collect::<Vec<_>>(),
        )?;
        ensure_no_permission_conflicts(
            "override permission",
            &self.allows_override,
            &self.disallow_override.iter().cloned().collect::<Vec<_>>(),
        )
    }

    pub fn apply(&self, source: &Type) -> Result<Type, AppError> {
        self.validate()?;

        let source_field_names = source
            .body
            .iter()
            .map(|field| field.name().to_string())
            .collect::<BTreeSet<_>>();
        let mut body = Vec::new();

        //A Existing fields are visited in declaration order so a replacement
        //A occupies the exact display position of the field it supersedes.
        for source_field in &source.body {
            if self.drop_fields.contains(source_field.name()) {
                continue;
            }
            let replacement = self
                .fields
                .iter()
                .find(|field| field.name() == source_field.name())
                .cloned()
                .unwrap_or_else(|| source_field.clone());
            body.push(replacement);
        }
        body.extend(
            self.fields
                .iter()
                .filter(|field| !source_field_names.contains(field.name()))
                .cloned(),
        );

        let candidate = Type {
            name: self.new_name.clone().unwrap_or_else(|| source.name.clone()),
            body: body.into_iter().collect(),
            outgoing: patch_permissions(
                &source.outgoing,
                &self.allow_outgoing,
                &self.disallow_outgoing,
            ),
            incoming: patch_permissions(
                &source.incoming,
                &self.allow_incoming,
                &self.disallow_incoming,
            ),
            additional_override: patch_permissions(
                &source.additional_override,
                &self.allows_override,
                &self.disallow_override,
            ),
        };

        if candidate == *source {
            return Err(AppError::type_patch_no_op(&source.name));
        }

        Ok(candidate)
    }
}

pub struct ResolvedTypeDeclaration {
    pub entry: ResolvedEntry,
    pub declaration: Type,
}

impl ResolvedTypeDeclaration {
    pub fn record(&self) -> TypeRecord {
        type_record(&self.entry, &self.declaration)
    }
}

pub struct TypeListing {
    pub entry: ResolvedEntry,
    pub declaration: Type,
}

impl TypeListing {
    pub fn record(&self) -> TypeRecord {
        type_record(&self.entry, &self.declaration)
    }
}

pub fn list_types(
    graph: &LifeCoreList,
    state: &CliStateV1,
    view: &GraphView,
) -> Result<Vec<TypeListing>, AppError> {
    //A Malformed stored Type nodes remain analyzer concerns. Read-only listing
    //A presents only the well-formed active declarations promised by SPEC.md.
    let mut declarations = graph
        .fresh_nodes(view.context())
        .filter(|node| node.ty_name.as_str() == Type::TYPE_NAME)
        .filter_map(|node| {
            Type::try_from_body(&node.field)
                .ok()
                .map(|declaration| (node.e.entry().0, declaration))
        })
        .collect::<Vec<_>>();
    declarations.sort_by(|left, right| {
        left.1
            .name
            .cmp(&right.1.name)
            .then_with(|| left.0.cmp(&right.0))
    });

    declarations
        .into_iter()
        .map(|(index, declaration)| {
            let entry = resolve_index(
                graph,
                state,
                index,
                EntryExpectation::Node,
                Some(view.end_exclusive()),
            )?;
            Ok(TypeListing { entry, declaration })
        })
        .collect()
}

pub fn resolve_active_type(
    graph: &LifeCoreList,
    state: &CliStateV1,
    view: &GraphView,
    name: &str,
) -> Result<ResolvedTypeDeclaration, AppError> {
    let matches = list_types(graph, state, view)?
        .into_iter()
        .filter(|listing| listing.declaration.name == name)
        .collect::<Vec<_>>();

    match matches.len() {
    0 => Err(AppError::type_declaration_not_found(name)),
    1 => {
        let listing = matches.into_iter().next().expect("one match exists");
        Ok(ResolvedTypeDeclaration {
            entry: listing.entry,
            declaration: listing.declaration,
        })
    }
    count => Err(AppError::ambiguous_type_declaration(name, count)),
    }
}

pub fn resolve_selected_type(
    graph: &LifeCoreList,
    view: &GraphView,
    entry: ResolvedEntry,
) -> Result<ResolvedTypeDeclaration, AppError> {
    decode_type_entry(graph, view, entry, true)
}

pub fn inspect_type(
    graph: &LifeCoreList,
    view: &GraphView,
    entry: ResolvedEntry,
) -> Result<ResolvedTypeDeclaration, AppError> {
    decode_type_entry(graph, view, entry, false)
}

pub fn plan_type_append(declaration: Type, parent: Option<EntryIndex>) -> PlannedChange {
    PlannedChange::AddNode {
        type_name: Type::TYPE_NAME.to_string(),
        body: declaration.into_body(),
        parent,
    }
}

pub fn render_type_declaration(declaration: &Type) -> String {
    let mut arguments = vec![
        "lifecore type create".to_string(),
        shell_quote(&declaration.name),
    ];

    for field in &declaration.body {
        arguments.push("--field".to_string());
        arguments.push(shell_quote(&field.to_string()));
    }
    for permission in &declaration.outgoing {
        arguments.push("--outgoing".to_string());
        arguments.push(shell_quote(permission));
    }
    for permission in &declaration.incoming {
        arguments.push("--incoming".to_string());
        arguments.push(shell_quote(permission));
    }
    for permission in &declaration.additional_override {
        arguments.push("--allows-override".to_string());
        arguments.push(shell_quote(permission));
    }

    let mut rendered = format!("{} {}", arguments[0], arguments[1]);
    for pair in arguments[2..].chunks_exact(2) {
        rendered.push_str(" \\\n  ");
        rendered.push_str(&pair.join(" "));
    }
    rendered
}

fn decode_type_entry(
    graph: &LifeCoreList,
    view: &GraphView,
    entry: ResolvedEntry,
    require_fresh: bool,
) -> Result<ResolvedTypeDeclaration, AppError> {
    let Entry::Node(node) = &entry.entry else {
        return Err(AppError::invalid_type_declaration(format!(
            "entry {} is not a node",
            entry.index()
        )));
    };
    if node.ty_name.as_str() != Type::TYPE_NAME {
        return Err(AppError::invalid_type_declaration(format!(
            "selected node {} has stored type {:?}; expected {:?}",
            entry.index(),
            node.ty_name,
            Type::TYPE_NAME
        )));
    }
    if require_fresh {
        //A Freshness is recomputed in the caller's selected view; the pointed
        //A context stored on the node may represent a different history window.
        let pointed = view.context().look_at(node.e.absolute());
        if graph.is_stale(pointed) != Some(false) {
            return Err(AppError::invalid_type_declaration(format!(
                "selected Type node {} is stale in the selected view",
                entry.index()
            )));
        }
    }
    let declaration = Type::try_from_body(&node.field).map_err(|error| {
        AppError::invalid_type_declaration(format!(
            "selected Type node {} could not be decoded: {error}",
            entry.index()
        ))
    })?;

    Ok(ResolvedTypeDeclaration { entry, declaration })
}

fn parse_field_declarations(
    values: &[String],
    operation: &str,
    duplicate_is_patch_error: bool,
) -> Result<Vec<FieldDecl>, AppError> {
    let mut names = BTreeSet::new();
    values
        .iter()
        .map(|value| {
            let field = value.parse::<FieldDecl>().map_err(|error| {
                AppError::invalid_type_declaration(format!(
                    "{operation} field {value:?} is invalid: {error}"
                ))
            })?;
            if !names.insert(field.name().to_string()) {
                let message = format!("field {:?} is specified more than once", field.name());
                return Err(if duplicate_is_patch_error {
                    AppError::invalid_type_patch(message)
                } else {
                    AppError::invalid_type_declaration(message)
                });
            }
            Ok(field)
        })
        .collect()
}

fn parse_field_name(value: &str) -> Result<String, AppError> {
    let probe = format!("{value} = Text : Optional");
    probe
        .parse::<FieldDecl>()
        .map(|field| field.name().to_string())
        .map_err(|error| {
            AppError::invalid_type_patch(format!("field name {value:?} is invalid: {error}"))
        })
}

fn ensure_no_field_conflicts(
    fields: &[FieldDecl],
    drop_fields: &BTreeSet<String>,
) -> Result<(), AppError> {
    if let Some(field) = fields
        .iter()
        .find(|field| drop_fields.contains(field.name()))
    {
        return Err(AppError::invalid_type_patch(format!(
            "field {:?} cannot be replaced and dropped in one command",
            field.name()
        )));
    }

    Ok(())
}

fn ensure_no_permission_conflicts(
    kind: &str,
    additions: &[String],
    removals: &[String],
) -> Result<(), AppError> {
    let removals = removals.iter().collect::<BTreeSet<_>>();
    if let Some(value) = additions.iter().find(|value| removals.contains(value)) {
        return Err(AppError::invalid_type_patch(format!(
            "{kind} {value:?} cannot be allowed and disallowed in one command"
        )));
    }

    Ok(())
}

fn patch_permissions(
    source: &OrderedSet<String>,
    additions: &[String],
    removals: &BTreeSet<String>,
) -> OrderedSet<String> {
    let mut patched = source
        .iter()
        .filter(|value| !removals.contains(*value))
        .cloned()
        .collect::<OrderedSet<_>>();
    patched.extend(additions.iter().cloned());
    patched
}

fn type_record(entry: &ResolvedEntry, declaration: &Type) -> TypeRecord {
    TypeRecord {
        index: entry.index(),
        name: declaration.name.clone(),
        locator: entry.full_locator(),
        display_locator: entry.display_locator(),
        fields: declaration.body.iter().map(ToString::to_string).collect(),
        outgoing: declaration.outgoing.iter().cloned().collect(),
        incoming: declaration.incoming.iter().cloned().collect(),
        allows_override: declaration.additional_override.iter().cloned().collect(),
        command: render_type_declaration(declaration),
    }
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}
