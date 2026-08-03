use crate::prelude::*;

use super::{PIPE_ATOMIC_WRITE_BYTES, PipeInner, PipeRx, PipeTx};
use crate::fs::iomux::PollRoute;

#[derive(Clone, Debug)]
pub(super) struct PipePollRoute {
    route: PollRoute,
    interests: PollEvent,
}

impl PipePollRoute {
    fn new(route: &PollRoute, interests: PollEvent) -> Self {
        Self {
            route: route.clone(),
            interests,
        }
    }
}

fn replace_pipe_poll_routes(
    routes: &mut Arc<Vec<PipePollRoute>>,
    route: &PollRoute,
    interests: PollEvent,
) -> Result<(Arc<Vec<PipePollRoute>>, usize), SysError> {
    let retained = routes
        .iter()
        .filter(|entry| !entry.route.is_prunable())
        .count();
    let capacity = retained.checked_add(1).ok_or(SysError::OutOfMemory)?;
    let mut replacement = Vec::new();
    replacement
        .try_reserve(capacity)
        .map_err(|_| SysError::OutOfMemory)?;
    replacement.extend(
        routes
            .iter()
            .filter(|entry| !entry.route.is_prunable())
            .cloned(),
    );
    let pruned = routes.len() - replacement.len();
    replacement.push(PipePollRoute::new(route, interests));

    let replacement = Arc::try_new(replacement).map_err(|_| SysError::OutOfMemory)?;
    Ok((core::mem::replace(routes, replacement), pruned))
}

pub(super) fn notify_pipe_poll_routes(
    routes: Option<Arc<Vec<PipePollRoute>>>,
    changed: Option<PollEvent>,
    side: &'static str,
    reason: &'static str,
) {
    let Some(routes) = routes else {
        return;
    };

    let mut candidates = 0usize;
    for entry in routes.iter() {
        if changed.is_none_or(|changed| entry.interests.intersects(changed)) {
            entry.route.notify();
            candidates += 1;
        }
    }
    if candidates > 0 {
        kdebugln!(
            "pipe: issued {} {} poll route hints reason={}",
            candidates,
            side,
            reason,
        );
    }
}

fn pipe_rx_revents(pipe: &PipeInner, interests: PollEvent) -> PollEvent {
    let mut revents = PollEvent::empty();
    if interests.contains(PollEvent::READABLE) && (!pipe.buf.is_empty() || pipe.tx_cnt == 0) {
        revents |= PollEvent::READABLE;
    }
    if pipe.tx_cnt == 0 {
        revents |= PollEvent::HANG_UP;
    }
    revents
}

fn pipe_tx_revents(pipe: &PipeInner, interests: PollEvent) -> PollEvent {
    let mut revents = PollEvent::empty();
    // A complete PIPE_BUF write is the source-owned WRITABLE predicate. Route
    // notification remains only a guard-out hint followed by a final recheck.
    if interests.contains(PollEvent::WRITABLE) && pipe.available() >= PIPE_ATOMIC_WRITE_BYTES {
        revents |= PollEvent::WRITABLE;
    }
    if pipe.rx_cnt == 0 {
        revents |= PollEvent::ERROR;
    }
    revents
}

pub(super) fn pipe_rx_poll(
    file: &File,
    request: &PollRequest<'_>,
) -> Result<PollRegisterResult, SysError> {
    let rx = file
        .prv()
        .cast::<PipeRx>()
        .expect("internal error: pipe rx file without correct private data");
    let mut pipe = rx.pipe.inner.lock();
    if !request.is_register() {
        return Ok(PollRegisterResult::Ready(pipe_rx_revents(
            &pipe,
            request.interests(),
        )));
    }
    let Some(route) = request.route() else {
        let revents = pipe_rx_revents(&pipe, request.interests());
        return Ok(if revents.is_empty() {
            PollRegisterResult::Unsupported
        } else {
            PollRegisterResult::Ready(revents)
        });
    };
    let (previous_routes, pruned) =
        replace_pipe_poll_routes(&mut pipe.rx_poll_routes, route, request.interests())?;
    let revents = pipe_rx_revents(&pipe, request.interests());
    let queue_len = pipe.rx_poll_routes.len();
    drop(pipe);
    drop(previous_routes);
    kdebugln!(
        "pipe: subscribed rx poll interests={:?} queue_len={} pruned={}",
        request.interests(),
        queue_len,
        pruned,
    );
    Ok(PollRegisterResult::Subscribed(revents))
}

pub(super) fn pipe_tx_poll(
    file: &File,
    request: &PollRequest<'_>,
) -> Result<PollRegisterResult, SysError> {
    let tx = file
        .prv()
        .cast::<PipeTx>()
        .expect("internal error: pipe tx file without correct private data");
    let mut pipe = tx.pipe.inner.lock();
    if !request.is_register() {
        return Ok(PollRegisterResult::Ready(pipe_tx_revents(
            &pipe,
            request.interests(),
        )));
    }
    let Some(route) = request.route() else {
        let revents = pipe_tx_revents(&pipe, request.interests());
        return Ok(if revents.is_empty() {
            PollRegisterResult::Unsupported
        } else {
            PollRegisterResult::Ready(revents)
        });
    };
    let (previous_routes, pruned) =
        replace_pipe_poll_routes(&mut pipe.tx_poll_routes, route, request.interests())?;
    let revents = pipe_tx_revents(&pipe, request.interests());
    let queue_len = pipe.tx_poll_routes.len();
    drop(pipe);
    drop(previous_routes);
    kdebugln!(
        "pipe: subscribed tx poll interests={:?} queue_len={} pruned={}",
        request.interests(),
        queue_len,
        pruned,
    );
    Ok(PollRegisterResult::Subscribed(revents))
}
