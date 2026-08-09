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
pub mod game;
pub mod modpack;

pub use deploy::{Conflict, DeployMethod, DeploymentPlan, PlannedFile, Selection, ValidationIssue};
pub use game::{
    ConflictScope, DeployTarget, Engine, GameProfile, LoadOrderPolicy, LoaderKind, LoaderSpec,
    PakChainSpec, ProtonLoaderSpec, RewrapRule, SteamDetection,
};
pub use modpack::{
    DeployRoot, FilePayload, InstallerModel, ModBundle, ModOption, OptionGroup, SelectMode,
};
