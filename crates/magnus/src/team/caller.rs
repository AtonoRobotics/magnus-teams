//! The member asking, resolved once before a workspace route runs
//! (decision `2026-11-14-handlers-take-a-caller.md`): `Member<at::Admin>`
//! is someone signed in, in the workspace the path's `{ws}` names, holding
//! at least its Admin role. It refuses as `routes::member` always did: a
//! 401 without a sign-in, a 404 that does not say the workspace exists, a
//! 403 naming the role it takes. What a member may then do is one call,
//! `team::may::allow`.

use super::Role;
use crate::api_error::ApiError;
use crate::config::Person;
use crate::kernel::Kernel;
use axum::extract::{FromRequestParts, RawPathParams};
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use std::marker::PhantomData;
use std::sync::Arc;

/// The least role a route takes.
pub trait Floor {
    const ROLE: Role;
}

/// The floors, by the role each names.
pub mod at {
    use super::{Floor, Role};

    pub struct Guest;
    pub struct Member;
    pub struct Admin;
    pub struct Owner;

    impl Floor for Guest {
        const ROLE: Role = Role::Guest;
    }
    impl Floor for Member {
        const ROLE: Role = Role::Member;
    }
    impl Floor for Admin {
        const ROLE: Role = Role::Admin;
    }
    impl Floor for Owner {
        const ROLE: Role = Role::Owner;
    }
}

/// A member of the path's workspace, with at least `F`'s role.
pub struct Member<F = at::Guest> {
    pub person: Person,
    pub role: Role,
    floor: PhantomData<F>,
}

impl<F: Floor> FromRequestParts<Arc<Kernel>> for Member<F> {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, k: &Arc<Kernel>) -> Result<Member<F>, ApiError> {
        let params = RawPathParams::from_request_parts(parts, k).await.map_err(|r| ApiError::from(r.into_response()))?;
        let Some(ws) = params.iter().find(|(name, _)| *name == "ws").map(|(_, v)| v.to_string()) else {
            return Err(ApiError::new(StatusCode::NOT_FOUND, "no such workspace"));
        };
        let (person, role) = super::routes::member(k, &parts.headers, &ws, F::ROLE).await?;
        Ok(Member { person, role, floor: PhantomData })
    }
}
