//! Magnus: an autonomous digital worker that ships as an appliance.
//!
//! Kernel pieces: the event log (`log`, `events`, `state`), the execution
//! boundary (`boundary`, `policy`, `vault`), and the run loop (`run`).
//! Everything outside the kernel talks to it through `api`, the adapter
//! contract (`adapters`), MCP (`apps`), or vsock (`sandbox`).

pub mod accounts;
pub mod adapters;
pub mod admin;
pub mod api;
pub mod app_tools;
pub mod apps;
pub mod attachment;
pub mod auto_review;
pub mod backup;
pub mod boundary;
pub mod brokered;
pub mod browser;
pub mod bundle;
pub mod catalog;
pub mod chat;
pub mod chat_actions;
pub mod cli;
pub mod commitments;
pub mod compose;
pub mod computer;
pub mod config;
pub mod conformance;
pub mod connectors;
pub mod control;
pub mod controls;
pub mod dev;
pub mod devices;
pub mod events;
pub mod grants;
pub mod identity;
pub mod inbox;
pub mod kernel;
pub mod knowledge;
pub mod knowledge_admin;
pub mod library;
pub mod local;
pub mod log;
pub mod login_codes;
pub mod mailer;
pub mod oidc;
pub mod openers;
pub mod ops_admin;
pub mod person_admin;
pub mod policy;
pub mod policy_admin;
pub mod proactive;
pub mod reactions;
pub mod recall;
pub mod relationship;
pub mod rules;
pub mod run;
pub mod safety;
pub mod sandbox;
pub mod settings;
pub mod spawn;
pub mod speech;
pub mod state;
pub mod sub_agents;
pub mod suggestions;
pub mod team;
pub mod tls;
pub mod tools;
mod tools_identity;
pub mod tools_ui;
pub mod totp;
pub mod trash;
pub mod ui;
pub mod ui_actions;
pub mod unattended;
pub mod update;
pub mod vault;
pub mod web;
