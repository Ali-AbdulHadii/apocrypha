//! Reading `fomod/ModuleConfig.xml`.
//!
//! Hand-written against a pull parser rather than derived, and the reason is
//! the files themselves. FOMOD has a published schema that almost nothing obeys
//! exactly: the same value arrives as an attribute from one authoring tool and
//! as a child element from another, enum values come in every casing, elements
//! appear in an order the schema does not describe, and documents ship with
//! UTF-8 or UTF-16 byte order marks. A derived deserializer fails on files that
//! every other mod manager installs happily. A loop that asks for what it wants
//! and skips what it does not recognise handles all of it, and can say
//! precisely what went wrong when something is genuinely broken.
//!
//! Nothing here touches the filesystem or resolves a path against an archive.
//! It turns bytes into [`FomodModule`]; deciding what those declarations mean
//! for a particular archive is the lowering step's job.

use crate::error::{ModEngineError, Result};
use apoc_domain::fomod::{
    CompositeDependency, ConditionalPattern, FileSpec, FileState, FomodModule, GroupKind,
    InstallStep, Plugin, PluginGroup, PluginTypeName, SortOrder, TypeDescriptor,
};
use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;
use std::collections::HashSet;

/// Nesting cap. `<dependencies>` may contain `<dependencies>` without limit in
/// the schema, and a hostile document should meet a refusal rather than the end
/// of the stack.
const MAX_DEPTH: usize = 64;

/// Size cap. Real manifests are tens of kilobytes; this is three orders of
/// magnitude of headroom and still bounds the memory a bad archive can claim.
const MAX_BYTES: usize = 8 * 1024 * 1024;

/// Parse a `ModuleConfig.xml`.
pub fn parse_module_config(bytes: &[u8]) -> Result<FomodModule> {
    if bytes.len() > MAX_BYTES {
        return Err(ModEngineError::FomodMalformed(format!(
            "the installer description is {} MB, which is far larger than any real one",
            bytes.len() / (1024 * 1024)
        )));
    }
    let text = decode(bytes)?;
    let mut reader = Reader::from_str(&text);
    reader.config_mut().trim_text(true);
    reader.config_mut().check_end_names = true;

    let mut module = FomodModule {
        name: String::new(),
        image: None,
        module_dependencies: None,
        required_install_files: Vec::new(),
        install_steps: Vec::new(),
        step_order: SortOrder::Explicit,
        conditional_installs: Vec::new(),
        warnings: Vec::new(),
    };

    let mut depth = 0usize;
    let mut steps_seen = 0usize;
    loop {
        match read_event(&mut reader)? {
            Event::DocType(_) => {
                // A document type declaration can name external entities, and
                // no manifest has any business carrying one. Refused rather
                // than ignored, since ignoring it means parsing a document
                // whose author expected different rules.
                return Err(ModEngineError::FomodMalformed(
                    "it carries a document type declaration, which Apocrypha will not process"
                        .to_string(),
                ));
            }
            Event::Start(e) => {
                depth += 1;
                guard_depth(depth)?;
                match local_name(&e).as_str() {
                    "modulename" => {
                        module.name = read_text(&mut reader, &e)?;
                        depth -= 1;
                    }
                    "moduleimage" => {
                        module.image = attr(&e, "path").filter(|p| !p.is_empty());
                        skip_element(&mut reader, &e)?;
                        depth -= 1;
                    }
                    "moduledependencies" => {
                        module.module_dependencies = Some(read_dependency(
                            &mut reader,
                            &e,
                            depth,
                            &mut module.warnings,
                        )?);
                        depth -= 1;
                    }
                    "requiredinstallfiles" => {
                        module.required_install_files = read_files(&mut reader, &e, depth)?;
                        depth -= 1;
                    }
                    "installsteps" => {
                        module.step_order = order_of(&e, &mut module.warnings);
                        module.install_steps = read_install_steps(
                            &mut reader,
                            &e,
                            depth,
                            &mut steps_seen,
                            &mut module.warnings,
                        )?;
                        depth -= 1;
                    }
                    "conditionalfileinstalls" => {
                        module.conditional_installs = read_conditional_installs(
                            &mut reader,
                            &e,
                            depth,
                            &mut module.warnings,
                        )?;
                        depth -= 1;
                    }
                    // `config`, and anything a future schema adds.
                    _ => {}
                }
            }
            Event::Empty(e) => {
                if local_name(&e) == "moduleimage" {
                    module.image = attr(&e, "path").filter(|p| !p.is_empty());
                }
            }
            Event::End(_) => depth = depth.saturating_sub(1),
            Event::Eof => {
                // Elements still open at the end of the file means the document
                // was cut short. quick-xml reports that as a clean end of
                // input, and taking it at face value would turn a half-written
                // installer into one that silently offers half its options.
                if depth > 0 {
                    return Err(ModEngineError::FomodMalformed(
                        "the XML is not well formed (it ends part-way through an element)"
                            .to_string(),
                    ));
                }
                break;
            }
            _ => {}
        }
    }

    if module.name.trim().is_empty() {
        module.name = "Unnamed mod".to_string();
        module
            .warnings
            .push("The installer does not give the mod a name.".to_string());
    }
    Ok(module)
}

/* ------------------------------------------------------------- elements --- */

fn read_install_steps(
    reader: &mut Reader<&[u8]>,
    parent: &BytesStart<'_>,
    depth: usize,
    steps_seen: &mut usize,
    warnings: &mut Vec<String>,
) -> Result<Vec<InstallStep>> {
    let end = parent.to_end().into_owned();
    let mut steps = Vec::new();
    let mut depth = depth;

    loop {
        match read_event(reader)? {
            Event::Start(e) => {
                depth += 1;
                guard_depth(depth)?;
                if local_name(&e) == "installstep" {
                    let id = format!("step-{steps_seen}");
                    *steps_seen += 1;
                    steps.push(read_install_step(reader, &e, depth, id, warnings)?);
                    depth -= 1;
                }
            }
            Event::End(e) if e.name() == end.name() => break,
            Event::End(_) => depth = depth.saturating_sub(1),
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(steps)
}

fn read_install_step(
    reader: &mut Reader<&[u8]>,
    parent: &BytesStart<'_>,
    depth: usize,
    id: String,
    warnings: &mut Vec<String>,
) -> Result<InstallStep> {
    let end = parent.to_end().into_owned();
    let mut step = InstallStep {
        name: attr(parent, "name").unwrap_or_else(|| "Options".to_string()),
        id,
        visible: None,
        groups: Vec::new(),
    };
    let mut depth = depth;
    let mut groups_seen = 0usize;

    loop {
        match read_event(reader)? {
            Event::Start(e) => {
                depth += 1;
                guard_depth(depth)?;
                match local_name(&e).as_str() {
                    "visible" => {
                        step.visible = Some(read_dependency(reader, &e, depth, warnings)?);
                        depth -= 1;
                    }
                    "group" => {
                        let id = format!("{}/group-{groups_seen}", step.id);
                        groups_seen += 1;
                        step.groups
                            .push(read_group(reader, &e, depth, id, warnings)?);
                        depth -= 1;
                    }
                    // `optionalFileGroups` is a container; falling through
                    // means its `<group>` children are read by this same loop.
                    _ => {}
                }
            }
            Event::End(e) if e.name() == end.name() => break,
            Event::End(_) => depth = depth.saturating_sub(1),
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(step)
}

fn read_group(
    reader: &mut Reader<&[u8]>,
    parent: &BytesStart<'_>,
    depth: usize,
    id: String,
    warnings: &mut Vec<String>,
) -> Result<PluginGroup> {
    let end = parent.to_end().into_owned();
    let name = attr(parent, "name").unwrap_or_else(|| "Options".to_string());
    let kind = match attr(parent, "type") {
        Some(raw) => group_kind(&raw).ok_or_else(|| {
            // The one enum where a fallback is not acceptable. Cardinality
            // decides whether the user may proceed having chosen nothing, and
            // guessing it either blocks a valid install or waves through one
            // the author said was incomplete.
            ModEngineError::FomodMalformed(format!(
                "the group \"{name}\" asks for a selection type of \"{raw}\", which is not one this format defines"
            ))
        })?,
        None => {
            warnings.push(format!(
                "The group \"{name}\" does not say how many options may be chosen. Any number is allowed."
            ));
            GroupKind::SelectAny
        }
    };

    let mut group = PluginGroup {
        id,
        name,
        kind,
        order: order_of(parent, warnings),
        plugins: Vec::new(),
    };
    let mut depth = depth;
    // Two plugins in one group may carry the same name, and two long distinct
    // names may share the first 48 characters the slug keeps. Either way the
    // slug repeats, and an id is what a selection names, so a repeat would make
    // choosing one option install every plugin that answers to the same id --
    // in a SelectExactlyOne group, exactly where exclusivity matters most.
    let mut taken: HashSet<String> = HashSet::new();

    loop {
        match read_event(reader)? {
            Event::Start(e) => {
                depth += 1;
                guard_depth(depth)?;
                if local_name(&e) == "plugin" {
                    let mut plugin = read_plugin(reader, &e, depth, warnings)?;
                    plugin.id = format!(
                        "{}/{}",
                        group.id,
                        unique_slug(&slug(&plugin.name), &mut taken)
                    );
                    group.plugins.push(plugin);
                    depth -= 1;
                }
            }
            Event::End(e) if e.name() == end.name() => break,
            Event::End(_) => depth = depth.saturating_sub(1),
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(group)
}

fn read_plugin(
    reader: &mut Reader<&[u8]>,
    parent: &BytesStart<'_>,
    depth: usize,
    warnings: &mut Vec<String>,
) -> Result<Plugin> {
    let end = parent.to_end().into_owned();
    let mut name = attr(parent, "name").unwrap_or_default();
    let mut plugin = Plugin {
        id: String::new(),
        name: String::new(),
        description: None,
        image: None,
        files: Vec::new(),
        condition_flags: Vec::new(),
        type_descriptor: TypeDescriptor::plain(PluginTypeName::Optional),
    };
    let mut depth = depth;

    loop {
        match read_event(reader)? {
            Event::Start(e) => {
                depth += 1;
                guard_depth(depth)?;
                match local_name(&e).as_str() {
                    // Some tools write the name as a child rather than an
                    // attribute. Both are the name.
                    "name" if name.is_empty() => {
                        name = read_text(reader, &e)?;
                        depth -= 1;
                    }
                    "description" => {
                        let text = read_text(reader, &e)?;
                        depth -= 1;
                        plugin.description = Some(text).filter(|t| !t.trim().is_empty());
                    }
                    "image" => {
                        plugin.image = attr(&e, "path").filter(|p| !p.is_empty());
                        skip_element(reader, &e)?;
                        depth -= 1;
                    }
                    "files" => {
                        plugin.files = read_files(reader, &e, depth)?;
                        depth -= 1;
                    }
                    "conditionflags" => {
                        plugin.condition_flags = read_condition_flags(reader, &e, depth)?;
                        depth -= 1;
                    }
                    "typedescriptor" => {
                        plugin.type_descriptor = read_type_descriptor(reader, &e, depth, warnings)?;
                        depth -= 1;
                    }
                    _ => {}
                }
            }
            Event::Empty(e) => {
                if local_name(&e) == "image" {
                    plugin.image = attr(&e, "path").filter(|p| !p.is_empty());
                }
            }
            Event::End(e) if e.name() == end.name() => break,
            Event::End(_) => depth = depth.saturating_sub(1),
            Event::Eof => break,
            _ => {}
        }
    }

    if name.trim().is_empty() {
        name = "Unnamed option".to_string();
    }
    // The id is composed by the caller, which is the only place that can see
    // the other plugins in this group and so the only place that can tell
    // whether a slug is already spoken for.
    plugin.name = name;
    Ok(plugin)
}

fn read_type_descriptor(
    reader: &mut Reader<&[u8]>,
    parent: &BytesStart<'_>,
    depth: usize,
    warnings: &mut Vec<String>,
) -> Result<TypeDescriptor> {
    let end = parent.to_end().into_owned();
    let mut descriptor = TypeDescriptor::plain(PluginTypeName::Optional);
    let mut depth = depth;
    let mut pending: Option<CompositeDependency> = None;

    loop {
        match read_event(reader)? {
            Event::Start(e) => {
                depth += 1;
                guard_depth(depth)?;
                match local_name(&e).as_str() {
                    "type" => {
                        descriptor.default = plugin_type(&e, warnings);
                        skip_element(reader, &e)?;
                        depth -= 1;
                    }
                    "defaulttype" => {
                        descriptor.default = plugin_type(&e, warnings);
                        skip_element(reader, &e)?;
                        depth -= 1;
                    }
                    "dependencies" => {
                        pending = Some(read_dependency(reader, &e, depth, warnings)?);
                        depth -= 1;
                    }
                    _ => {}
                }
            }
            Event::Empty(e) => match local_name(&e).as_str() {
                "type" | "defaulttype" => descriptor.default = plugin_type(&e, warnings),
                _ => {}
            },
            Event::End(e) if e.name() == end.name() => break,
            Event::End(e) => {
                depth = depth.saturating_sub(1);
                // A `<pattern>` closes once its dependencies and its type have
                // both been read; the type is whatever `<type>` last set, so it
                // is taken here and the default restored afterwards.
                if local_name_bytes(e.name().as_ref()) == "pattern" {
                    if let Some(condition) = pending.take() {
                        descriptor.patterns.push((condition, descriptor.default));
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(descriptor)
}

fn read_condition_flags(
    reader: &mut Reader<&[u8]>,
    parent: &BytesStart<'_>,
    depth: usize,
) -> Result<Vec<(String, String)>> {
    let end = parent.to_end().into_owned();
    let mut flags = Vec::new();
    let mut depth = depth;

    loop {
        match read_event(reader)? {
            Event::Start(e) => {
                depth += 1;
                guard_depth(depth)?;
                if local_name(&e) == "flag" {
                    let name = attr(&e, "name").unwrap_or_default();
                    let value = read_text(reader, &e)?;
                    depth -= 1;
                    if !name.is_empty() {
                        flags.push((name, value));
                    }
                }
            }
            Event::Empty(e) => {
                // `<flag name="X"/>` sets the flag to the empty string, which
                // is a real value and is how "unset" is usually expressed.
                if local_name(&e) == "flag" {
                    if let Some(name) = attr(&e, "name") {
                        flags.push((name, String::new()));
                    }
                }
            }
            Event::End(e) if e.name() == end.name() => break,
            Event::End(_) => depth = depth.saturating_sub(1),
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(flags)
}

fn read_files(
    reader: &mut Reader<&[u8]>,
    parent: &BytesStart<'_>,
    depth: usize,
) -> Result<Vec<FileSpec>> {
    let end = parent.to_end().into_owned();
    let mut files = Vec::new();
    let mut depth = depth;

    loop {
        match read_event(reader)? {
            Event::Start(e) => {
                depth += 1;
                guard_depth(depth)?;
                if let Some(spec) = file_spec(&e) {
                    files.push(spec);
                    skip_element(reader, &e)?;
                    depth -= 1;
                }
            }
            Event::Empty(e) => {
                if let Some(spec) = file_spec(&e) {
                    files.push(spec);
                }
            }
            Event::End(e) if e.name() == end.name() => break,
            Event::End(_) => depth = depth.saturating_sub(1),
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(files)
}

fn file_spec(e: &BytesStart<'_>) -> Option<FileSpec> {
    let is_folder = match local_name(e).as_str() {
        "file" => false,
        "folder" => true,
        _ => return None,
    };
    let source = attr(e, "source")?;
    Some(FileSpec {
        // Backslashes are what a Windows authoring tool writes. Normalised here
        // so nothing downstream has to remember which kind of separator it has.
        source: source.replace('\\', "/"),
        destination: attr(e, "destination").map(|d| d.replace('\\', "/")),
        priority: attr(e, "priority")
            .and_then(|p| p.trim().parse::<i32>().ok())
            .unwrap_or(0),
        is_folder,
        always_install: flag_attr(e, "alwaysInstall"),
        install_if_usable: flag_attr(e, "installIfUsable"),
    })
}

fn read_conditional_installs(
    reader: &mut Reader<&[u8]>,
    parent: &BytesStart<'_>,
    depth: usize,
    warnings: &mut Vec<String>,
) -> Result<Vec<ConditionalPattern>> {
    let end = parent.to_end().into_owned();
    let mut patterns: Vec<ConditionalPattern> = Vec::new();
    let mut depth = depth;
    let mut condition: Option<CompositeDependency> = None;
    let mut files: Vec<FileSpec> = Vec::new();

    loop {
        match read_event(reader)? {
            Event::Start(e) => {
                depth += 1;
                guard_depth(depth)?;
                match local_name(&e).as_str() {
                    "dependencies" => {
                        condition = Some(read_dependency(reader, &e, depth, warnings)?);
                        depth -= 1;
                    }
                    "files" => {
                        files = read_files(reader, &e, depth)?;
                        depth -= 1;
                    }
                    _ => {}
                }
            }
            Event::End(e) if e.name() == end.name() => break,
            Event::End(e) => {
                depth = depth.saturating_sub(1);
                if local_name_bytes(e.name().as_ref()) == "pattern" {
                    if let Some(c) = condition.take() {
                        patterns.push(ConditionalPattern {
                            id: format!("@conditional-{}", patterns.len()),
                            condition: c,
                            files: std::mem::take(&mut files),
                        });
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(patterns)
}

/* ---------------------------------------------------------- dependencies --- */

fn read_dependency(
    reader: &mut Reader<&[u8]>,
    parent: &BytesStart<'_>,
    depth: usize,
    warnings: &mut Vec<String>,
) -> Result<CompositeDependency> {
    guard_depth(depth)?;
    let end = parent.to_end().into_owned();
    let any = attr(parent, "operator")
        .map(|op| op.eq_ignore_ascii_case("or"))
        .unwrap_or(false);
    let mut parts: Vec<CompositeDependency> = Vec::new();
    let mut depth = depth;

    loop {
        match read_event(reader)? {
            Event::Start(e) => {
                depth += 1;
                guard_depth(depth)?;
                match dependency_of(&e, warnings) {
                    Some(dep) => {
                        parts.push(dep);
                        skip_element(reader, &e)?;
                        depth -= 1;
                    }
                    None if local_name(&e) == "dependencies" => {
                        parts.push(read_dependency(reader, &e, depth, warnings)?);
                        depth -= 1;
                    }
                    None => {}
                }
            }
            Event::Empty(e) => {
                if let Some(dep) = dependency_of(&e, warnings) {
                    parts.push(dep);
                }
            }
            Event::End(e) if e.name() == end.name() => break,
            Event::End(_) => depth = depth.saturating_sub(1),
            Event::Eof => break,
            _ => {}
        }
    }

    Ok(if any {
        CompositeDependency::Or(parts)
    } else {
        CompositeDependency::And(parts)
    })
}

fn dependency_of(e: &BytesStart<'_>, warnings: &mut Vec<String>) -> Option<CompositeDependency> {
    match local_name(e).as_str() {
        "flagdependency" => Some(CompositeDependency::Flag {
            name: attr(e, "flag").unwrap_or_default(),
            value: attr(e, "value").unwrap_or_default(),
        }),
        "filedependency" => {
            let path = attr(e, "file").unwrap_or_default().replace('\\', "/");
            let state = match attr(e, "state")
                .unwrap_or_default()
                .to_ascii_lowercase()
                .as_str()
            {
                "missing" => FileState::Missing,
                "inactive" => FileState::Inactive,
                _ => FileState::Active,
            };
            Some(CompositeDependency::File { path, state })
        }
        "gamedependency" => Some(CompositeDependency::GameVersion(
            attr(e, "version").unwrap_or_default(),
        )),
        "fommdependency" => Some(CompositeDependency::ManagerVersion(
            attr(e, "version").unwrap_or_default(),
        )),
        // A condition kind this build has never seen. Recorded as undecidable
        // rather than dropped, because dropping it would quietly widen or
        // narrow whatever the author was gating.
        other if other.ends_with("dependency") => {
            warnings.push(format!(
                "The installer uses a condition of type \"{other}\", which Apocrypha cannot check."
            ));
            Some(CompositeDependency::Undecidable(other.to_string()))
        }
        _ => None,
    }
}

/* ---------------------------------------------------------------- atoms --- */

fn group_kind(raw: &str) -> Option<GroupKind> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "selectexactlyone" => Some(GroupKind::SelectExactlyOne),
        "selectatmostone" => Some(GroupKind::SelectAtMostOne),
        "selectatleastone" => Some(GroupKind::SelectAtLeastOne),
        "selectall" => Some(GroupKind::SelectAll),
        "selectany" => Some(GroupKind::SelectAny),
        _ => None,
    }
}

fn plugin_type(e: &BytesStart<'_>, warnings: &mut Vec<String>) -> PluginTypeName {
    let raw = attr(e, "name").unwrap_or_default();
    match raw.trim().to_ascii_lowercase().as_str() {
        "required" => PluginTypeName::Required,
        "optional" => PluginTypeName::Optional,
        "recommended" => PluginTypeName::Recommended,
        "notusable" => PluginTypeName::NotUsable,
        "couldbeusable" => PluginTypeName::CouldBeUsable,
        _ => {
            // Unlike group cardinality, an unknown plugin type is recoverable:
            // treating it as optional offers the choice instead of making it,
            // and the user is told the manifest said something else. Quoted as
            // the author wrote it, so it can be found in the file.
            warnings.push(format!(
                "An option is described as \"{raw}\", which Apocrypha does not recognise. It is offered as an optional choice."
            ));
            PluginTypeName::Optional
        }
    }
}

fn order_of(e: &BytesStart<'_>, warnings: &mut Vec<String>) -> SortOrder {
    match attr(e, "order") {
        None => SortOrder::Explicit,
        Some(raw) => match raw.trim().to_ascii_lowercase().as_str() {
            "explicit" => SortOrder::Explicit,
            "ascending" => SortOrder::Ascending,
            "descending" => SortOrder::Descending,
            _ => {
                warnings.push(format!(
                    "The installer asks for \"{raw}\" ordering, which is not one this format defines. Document order is used."
                ));
                SortOrder::Explicit
            }
        },
    }
}

/// An attribute's value, matched without regard to case.
fn attr(e: &BytesStart<'_>, name: &str) -> Option<String> {
    e.attributes().flatten().find_map(|a| {
        let key = local_name_bytes(a.key.as_ref());
        if key.eq_ignore_ascii_case(name) {
            Some(String::from_utf8_lossy(a.value.as_ref()).into_owned())
        } else {
            None
        }
    })
}

/// A boolean attribute, absent meaning false.
fn flag_attr(e: &BytesStart<'_>, name: &str) -> bool {
    attr(e, name)
        .map(|v| {
            let v = v.trim().to_ascii_lowercase();
            v == "true" || v == "1"
        })
        .unwrap_or(false)
}

/// An element's name without any namespace prefix, lowercased.
fn local_name(e: &BytesStart<'_>) -> String {
    local_name_bytes(e.name().as_ref()).to_ascii_lowercase()
}

fn local_name_bytes(raw: &[u8]) -> String {
    let name = String::from_utf8_lossy(raw);
    name.rsplit(':')
        .next()
        .unwrap_or(&name)
        .to_ascii_lowercase()
}

/// The text inside an element, consuming through its close tag.
fn read_text(reader: &mut Reader<&[u8]>, start: &BytesStart<'_>) -> Result<String> {
    let end = start.to_end().into_owned();
    let mut text = String::new();
    loop {
        match read_event(reader)? {
            Event::Text(t) => {
                text.push_str(&t.decode().map_err(malformed)?);
            }
            Event::CData(c) => {
                text.push_str(&String::from_utf8_lossy(c.as_ref()));
            }
            Event::End(e) if e.name() == end.name() => break,
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(text.trim().to_string())
}

/// Consume an element's subtree without interpreting it.
fn skip_element(reader: &mut Reader<&[u8]>, start: &BytesStart<'_>) -> Result<()> {
    let end = start.to_end().into_owned();
    let mut inner = 0usize;
    loop {
        match read_event(reader)? {
            Event::Start(e) => {
                if e.name() == end.name() {
                    inner += 1;
                }
                guard_depth(inner)?;
            }
            Event::End(e) if e.name() == end.name() => {
                if inner == 0 {
                    break;
                }
                inner -= 1;
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(())
}

fn read_event<'a>(reader: &mut Reader<&'a [u8]>) -> Result<Event<'a>> {
    reader.read_event().map_err(malformed)
}

fn malformed(e: impl std::fmt::Display) -> ModEngineError {
    ModEngineError::FomodMalformed(format!("the XML is not well formed ({e})"))
}

fn guard_depth(depth: usize) -> Result<()> {
    if depth > MAX_DEPTH {
        return Err(ModEngineError::FomodMalformed(format!(
            "it nests elements more than {MAX_DEPTH} deep"
        )));
    }
    Ok(())
}

/// Lowercase, runs of anything else collapsed to a single dash.
///
/// Restricted deliberately: an option id becomes a staging directory name, and
/// a charset this narrow cannot produce two distinct ids that sanitise to the
/// same folder. That is an argument about sanitising, and it says nothing about
/// two plugins sharing a name or a 48-character prefix, which is what
/// [`unique_slug`] exists to settle.
fn slug(name: &str) -> String {
    let mut out = String::new();
    let mut dash = false;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
    }
    let trimmed = out.trim_end_matches('-');
    if trimmed.is_empty() {
        "option".to_string()
    } else {
        trimmed.chars().take(48).collect()
    }
}

/// The first free `slug`, `slug-2`, `slug-3`, ... within one group.
///
/// An ordinal rather than a hash so a duplicate stays readable, and appended
/// rather than woven in so the common case -- no duplicate at all -- keeps the
/// exact id it had before, and a saved selection still matches after upgrade.
/// The suffix is the plugin's position among its namesakes, not among all
/// plugins, so adding an unrelated option earlier in the group does not
/// renumber it.
fn unique_slug(slug: &str, taken: &mut HashSet<String>) -> String {
    if taken.insert(slug.to_string()) {
        return slug.to_string();
    }
    // Bounded by the plugins actually read, which the document size cap limits.
    for n in 2.. {
        let candidate = format!("{slug}-{n}");
        if taken.insert(candidate.clone()) {
            return candidate;
        }
    }
    unreachable!("the loop returns on the first free candidate")
}

/// Bytes to text, tolerating the encodings these files actually ship in.
fn decode(bytes: &[u8]) -> Result<String> {
    // UTF-8 with a byte order mark, which Windows editors add silently.
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return Ok(String::from_utf8_lossy(rest).into_owned());
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return Ok(utf16(rest, u16::from_le_bytes));
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return Ok(utf16(rest, u16::from_be_bytes));
    }
    // Lossy rather than an error. The only casualty is a mis-encoded character
    // in a description; every path is re-resolved against the archive's own
    // entries, which are the authority on how they are spelled.
    Ok(String::from_utf8_lossy(bytes).into_owned())
}

fn utf16(bytes: &[u8], to_unit: fn([u8; 2]) -> u16) -> String {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| to_unit([c[0], c[1]]))
        .collect();
    String::from_utf16_lossy(&units)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(xml: &str) -> FomodModule {
        parse_module_config(xml.as_bytes()).expect("parses")
    }

    fn error(xml: &str) -> String {
        parse_module_config(xml.as_bytes())
            .expect_err("should have been refused")
            .to_string()
    }

    const ONE_OF_EACH: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<config>
  <moduleName>Test Mod</moduleName>
  <moduleImage path="fomod\cover.jpg"/>
  <requiredInstallFiles>
    <file source="core\core.esp" destination="core.esp"/>
  </requiredInstallFiles>
  <installSteps order="Explicit">
    <installStep name="Body">
      <optionalFileGroups order="Explicit">
        <group name="Body type" type="SelectExactlyOne">
          <plugins order="Explicit">
            <plugin name="Slim">
              <description>A slimmer body.</description>
              <image path="fomod\slim.png"/>
              <files>
                <folder source="slim\meshes" destination="meshes" priority="2"/>
              </files>
              <conditionFlags>
                <flag name="body">slim</flag>
              </conditionFlags>
              <typeDescriptor><type name="Recommended"/></typeDescriptor>
            </plugin>
            <plugin name="Muscular">
              <files>
                <folder source="muscular\meshes" destination="meshes"/>
              </files>
              <typeDescriptor><type name="Optional"/></typeDescriptor>
            </plugin>
          </plugins>
        </group>
      </optionalFileGroups>
    </installStep>
  </installSteps>
</config>"#;

    #[test]
    fn a_whole_installer_reads_back_the_way_it_was_written() {
        let m = parse(ONE_OF_EACH);

        assert_eq!(m.name, "Test Mod");
        assert_eq!(m.image.as_deref(), Some("fomod\\cover.jpg"));
        assert_eq!(m.required_install_files.len(), 1);
        assert_eq!(m.required_install_files[0].source, "core/core.esp");

        let step = &m.install_steps[0];
        assert_eq!(step.name, "Body");
        let group = &step.groups[0];
        assert_eq!(group.kind, GroupKind::SelectExactlyOne);
        assert_eq!(group.plugins.len(), 2);

        let slim = &group.plugins[0];
        assert_eq!(slim.name, "Slim");
        assert_eq!(slim.description.as_deref(), Some("A slimmer body."));
        assert_eq!(slim.image.as_deref(), Some("fomod\\slim.png"));
        assert_eq!(slim.type_descriptor.default, PluginTypeName::Recommended);
        assert_eq!(slim.condition_flags, vec![("body".into(), "slim".into())]);

        let folder = &slim.files[0];
        assert!(folder.is_folder);
        // Backslashes are what the authoring tool wrote; nothing downstream
        // should have to remember that.
        assert_eq!(folder.source, "slim/meshes");
        assert_eq!(folder.destination.as_deref(), Some("meshes"));
        assert_eq!(folder.priority, 2);
    }

    #[test]
    fn every_group_kind_is_understood_whatever_its_casing() {
        for (written, expected) in [
            ("SelectExactlyOne", GroupKind::SelectExactlyOne),
            ("selectatmostone", GroupKind::SelectAtMostOne),
            ("SELECTATLEASTONE", GroupKind::SelectAtLeastOne),
            ("SelectAll", GroupKind::SelectAll),
            ("sElEcTaNy", GroupKind::SelectAny),
        ] {
            let m = parse(&format!(
                r#"<config><moduleName>M</moduleName><installSteps><installStep name="S">
                   <group name="G" type="{written}"><plugin name="P"/></group>
                   </installStep></installSteps></config>"#
            ));
            assert_eq!(m.install_steps[0].groups[0].kind, expected, "{written}");
        }
    }

    #[test]
    fn an_unknown_selection_type_is_refused_rather_than_guessed() {
        // Cardinality decides whether someone may proceed having chosen
        // nothing. Guessing it either blocks a valid install or waves through
        // one the author called incomplete.
        let msg = error(
            r#"<config><moduleName>M</moduleName><installSteps><installStep name="S">
               <group name="Body" type="SelectSome"><plugin name="P"/></group>
               </installStep></installSteps></config>"#,
        );
        assert!(msg.contains("Body"), "names the group: {msg}");
        assert!(msg.contains("SelectSome"), "quotes what it read: {msg}");
    }

    #[test]
    fn an_unknown_plugin_type_offers_the_choice_and_says_so() {
        let m = parse(
            r#"<config><moduleName>M</moduleName><installSteps><installStep name="S">
               <group name="G" type="SelectAny">
                 <plugin name="P"><typeDescriptor><type name="Mandatory"/></typeDescriptor></plugin>
               </group></installStep></installSteps></config>"#,
        );
        let plugin = &m.install_steps[0].groups[0].plugins[0];
        assert_eq!(plugin.type_descriptor.default, PluginTypeName::Optional);
        assert!(m.warnings.iter().any(|w| w.contains("Mandatory")));
    }

    #[test]
    fn a_name_written_as_a_child_element_is_still_the_name() {
        // Which of the two an authoring tool emits is not something the format
        // decides consistently.
        let m = parse(
            r#"<config><moduleName>M</moduleName><installSteps><installStep name="S">
               <group name="G" type="SelectAny">
                 <plugin><name>Written As A Child</name></plugin>
               </group></installStep></installSteps></config>"#,
        );
        assert_eq!(
            m.install_steps[0].groups[0].plugins[0].name,
            "Written As A Child"
        );
    }

    #[test]
    fn conditions_nest_and_keep_their_operator() {
        let m = parse(
            r#"<config><moduleName>M</moduleName>
               <installSteps><installStep name="Extras">
                 <visible>
                   <dependencies operator="And">
                     <flagDependency flag="body" value="slim"/>
                     <dependencies operator="Or">
                       <fileDependency file="SKSE64\skse.dll" state="Active"/>
                       <gameDependency version="1.6.640"/>
                     </dependencies>
                   </dependencies>
                 </visible>
                 <group name="G" type="SelectAny"><plugin name="P"/></group>
               </installStep></installSteps></config>"#,
        );

        let visible = m.install_steps[0]
            .visible
            .as_ref()
            .expect("has a condition");
        let CompositeDependency::And(outer) = visible else {
            panic!("outer condition should be a conjunction: {visible:?}");
        };
        // The <visible> wrapper contributes one level, the <dependencies>
        // inside it another.
        let CompositeDependency::And(parts) = &outer[0] else {
            panic!("expected the declared And: {:?}", outer[0]);
        };
        assert!(matches!(
            &parts[0],
            CompositeDependency::Flag { name, value } if name == "body" && value == "slim"
        ));
        let CompositeDependency::Or(inner) = &parts[1] else {
            panic!("expected a nested Or: {:?}", parts[1]);
        };
        assert!(matches!(
            &inner[0],
            CompositeDependency::File { path, state }
                if path == "SKSE64/skse.dll" && *state == FileState::Active
        ));
        assert!(matches!(&inner[1], CompositeDependency::GameVersion(v) if v == "1.6.640"));
    }

    #[test]
    fn a_condition_kind_this_build_has_never_seen_stays_undecidable() {
        // Dropping it would quietly widen or narrow whatever was being gated.
        let m = parse(
            r#"<config><moduleName>M</moduleName><installSteps><installStep name="S">
               <visible><dependencies><scriptExtenderDependency version="2.0"/></dependencies></visible>
               <group name="G" type="SelectAny"><plugin name="P"/></group>
               </installStep></installSteps></config>"#,
        );
        let visible = m.install_steps[0]
            .visible
            .as_ref()
            .expect("has a condition");
        let found = format!("{visible:?}");
        assert!(found.contains("Undecidable"), "{found}");
        assert!(m.warnings.iter().any(|w| w.contains("cannot check")));
    }

    #[test]
    fn conditional_file_installs_keep_their_condition_and_their_files() {
        let m = parse(
            r#"<config><moduleName>M</moduleName>
               <conditionalFileInstalls><patterns>
                 <pattern>
                   <dependencies><flagDependency flag="body" value="slim"/></dependencies>
                   <files><file source="patches\slim.esp"/></files>
                 </pattern>
               </patterns></conditionalFileInstalls></config>"#,
        );
        assert_eq!(m.conditional_installs.len(), 1);
        assert_eq!(m.conditional_installs[0].id, "@conditional-0");
        assert_eq!(
            m.conditional_installs[0].files[0].source,
            "patches/slim.esp"
        );
    }

    #[test]
    fn two_plugins_with_one_name_still_get_two_ids() {
        // An id is what a selection names. If both answer to the same one,
        // choosing either deploys both, and in a SelectExactlyOne group that
        // is the guarantee the group exists to make.
        let m = parse(
            r#"<config><moduleName>M</moduleName><installSteps><installStep name="S">
               <group name="G" type="SelectExactlyOne">
                 <plugin name="Slim"><files><file source="a.esp"/></files></plugin>
                 <plugin name="Slim"><files><file source="b.esp"/></files></plugin>
               </group></installStep></installSteps></config>"#,
        );
        let ids: Vec<&str> = m.install_steps[0].groups[0]
            .plugins
            .iter()
            .map(|p| p.id.as_str())
            .collect();
        assert_eq!(ids, ["step-0/group-0/slim", "step-0/group-0/slim-2"]);
    }

    #[test]
    fn names_sharing_the_first_48_characters_still_get_two_ids() {
        // The slug truncates, so distinct names can meet after it.
        let prefix = "a".repeat(48);
        let xml = format!(
            r#"<config><moduleName>M</moduleName><installSteps><installStep name="S">
               <group name="G" type="SelectExactlyOne">
                 <plugin name="{prefix}one"><files><file source="a.esp"/></files></plugin>
                 <plugin name="{prefix}two"><files><file source="b.esp"/></files></plugin>
               </group></installStep></installSteps></config>"#
        );
        let m = parse(&xml);
        let plugins = &m.install_steps[0].groups[0].plugins;
        assert_ne!(plugins[0].id, plugins[1].id);
    }

    #[test]
    fn a_plugin_with_no_namesake_keeps_the_id_it_always_had() {
        // The suffix is only ever added to a repeat, so a saved selection made
        // by an earlier build still matches.
        let m = parse(
            r#"<config><moduleName>M</moduleName><installSteps><installStep name="S">
               <group name="G" type="SelectAny">
                 <plugin name="Slim"><files><file source="a.esp"/></files></plugin>
                 <plugin name="Curvy"><files><file source="b.esp"/></files></plugin>
               </group></installStep></installSteps></config>"#,
        );
        let ids: Vec<&str> = m.install_steps[0].groups[0]
            .plugins
            .iter()
            .map(|p| p.id.as_str())
            .collect();
        assert_eq!(ids, ["step-0/group-0/slim", "step-0/group-0/curvy"]);
    }

    #[test]
    fn one_name_repeated_in_a_second_group_is_not_a_collision() {
        // The group id already separates them, so numbering is per group.
        let m = parse(
            r#"<config><moduleName>M</moduleName><installSteps><installStep name="S">
               <group name="A" type="SelectAny">
                 <plugin name="Slim"><files><file source="a.esp"/></files></plugin>
               </group>
               <group name="B" type="SelectAny">
                 <plugin name="Slim"><files><file source="b.esp"/></files></plugin>
               </group>
               </installStep></installSteps></config>"#,
        );
        let groups = &m.install_steps[0].groups;
        assert_eq!(groups[0].plugins[0].id, "step-0/group-0/slim");
        assert_eq!(groups[1].plugins[0].id, "step-0/group-1/slim");
    }

    #[test]
    fn a_document_type_declaration_is_refused() {
        let msg = error(
            r#"<?xml version="1.0"?><!DOCTYPE config [<!ENTITY x "y">]><config><moduleName>M</moduleName></config>"#,
        );
        assert!(msg.contains("document type declaration"), "{msg}");
    }

    #[test]
    fn a_deeply_nested_document_is_refused_rather_than_exhausting_the_stack() {
        let mut xml = String::from("<config><moduleName>M</moduleName>");
        for _ in 0..(MAX_DEPTH + 8) {
            xml.push_str("<dependencies>");
        }
        for _ in 0..(MAX_DEPTH + 8) {
            xml.push_str("</dependencies>");
        }
        xml.push_str("</config>");
        assert!(error(&xml).contains("nests elements"));
    }

    #[test]
    fn an_enormous_document_is_refused_before_it_is_read() {
        let big = vec![b' '; MAX_BYTES + 1];
        assert!(parse_module_config(&big)
            .expect_err("refused")
            .to_string()
            .contains("larger than any real one"));
    }

    #[test]
    fn a_byte_order_mark_does_not_stop_the_document_parsing() {
        let mut utf8 = vec![0xEF, 0xBB, 0xBF];
        utf8.extend_from_slice(b"<config><moduleName>Marked</moduleName></config>");
        assert_eq!(parse_module_config(&utf8).expect("parses").name, "Marked");

        let mut utf16le = vec![0xFF, 0xFE];
        for unit in "<config><moduleName>Wide</moduleName></config>".encode_utf16() {
            utf16le.extend_from_slice(&unit.to_le_bytes());
        }
        assert_eq!(parse_module_config(&utf16le).expect("parses").name, "Wide");
    }

    #[test]
    fn a_namespaced_document_is_read_the_same_as_a_bare_one() {
        let m = parse(
            r#"<f:config xmlns:f="http://qconsulting.ca/fo3/ModConfig5.0">
                 <f:moduleName>Namespaced</f:moduleName>
               </f:config>"#,
        );
        assert_eq!(m.name, "Namespaced");
    }

    #[test]
    fn an_installer_with_no_name_is_given_one_and_the_gap_is_reported() {
        let m = parse("<config><installSteps/></config>");
        assert_eq!(m.name, "Unnamed mod");
        assert!(m
            .warnings
            .iter()
            .any(|w| w.contains("does not give the mod a name")));
    }

    #[test]
    fn truncated_xml_reports_rather_than_panicking() {
        let msg = error("<config><moduleName>Half a docum");
        assert!(msg.contains("not well formed"), "{msg}");
    }

    #[test]
    fn option_ids_are_stable_slugs_that_cannot_collide_in_one_directory() {
        // Ids become staging directory names. This charset cannot produce two
        // distinct ids that sanitise to the same folder.
        assert_eq!(slug("Slim Body (2K)"), "slim-body-2k");
        assert_eq!(slug("---"), "option");
        assert_eq!(slug("a/b"), "a-b");
        assert_eq!(slug("a_b"), "a-b");
        assert!(slug(&"x".repeat(80)).len() <= 48);
    }
}
