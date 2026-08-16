//! Pure domain types for Apocrypha, the Linux-first desktop mod manager.
//!
//! This crate has **zero I/O and zero game-specific logic**. It is the shared
//! vocabulary that every other crate (`apoc-gamedef`, `apoc-modengine`,
//! `apoc-storage`, `apoc-deploy`, ...) speaks. Keeping it pure is what lets the
//! game-plugin architecture hold: a `GameProfile` is *data*, and the engines are
//! *mechanisms* that operate on these types.
//!
//! See `docs/desktop-manager-enhanced-brief.md` for the design this implements.

pub mod deploy;
pub mod fomod;
pub mod game;
pub mod modpack;
pub mod plugins;

pub use deploy::{Conflict, DeployMethod, DeploymentPlan, PlannedFile, Selection, ValidationIssue};
pub use fomod::{
    CompositeDependency, ConditionalPattern, FileSpec, FileState, FomodModule, GroupKind,
    InstallStep, Plugin, PluginGroup, PluginTypeName, SortOrder, Truth, TypeDescriptor,
};
pub use game::{
    ConflictScope, DeployTarget, Engine, FomodSpec, GameProfile, LoadOrderPolicy, LoaderKind,
    LoaderSpec, PakChainSpec, PluginActivation, PluginListSpec, ProtonLoaderSpec, RewrapRule,
    RootFilesSpec, SteamDetection,
};
pub use modpack::{
    DeployRoot, FilePayload, InstallerModel, ModBundle, ModOption, OptionGroup, SelectMode,
};
// `Plugin` is deliberately not re-exported here: `fomod::Plugin` already holds
// that name at the root, and a FOMOD option and a Creation Engine plugin are
// different things that would be one import away from being confused.
pub use plugins::{MasterProblem, MasterViolation, PluginEntry, PluginKind, PluginOrder};
