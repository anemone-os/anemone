use crate::prelude::*;

use super::{PIPE_ATOMIC_WRITE_BYTES, PipeEndpoint, PipeInner};
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

fn prepare_pipe_poll_routes(
    routes: &Arc<Vec<PipePollRoute>>,
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
    Ok((replacement, pruned))
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

fn pipe_rx_revents(
    pipe: &PipeInner,
    initial_no_writer_generation: Option<u64>,
    interests: PollEvent,
) -> PollEvent {
    let mut revents = PollEvent::empty();
    let visible_eof = pipe.tx_cnt == 0
        && initial_no_writer_generation.is_none_or(|initial| pipe.tx_generation != initial);
    if interests.contains(PollEvent::READABLE) && (!pipe.buf.is_empty() || visible_eof) {
        revents |= PollEvent::READABLE;
    }
    if visible_eof {
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

fn pipe_revents(pipe: &PipeInner, endpoint: &PipeEndpoint, interests: PollEvent) -> PollEvent {
    let mut revents = PollEvent::empty();
    if endpoint.access.can_read() {
        revents |= pipe_rx_revents(pipe, endpoint.initial_no_writer_generation, interests);
    }
    if endpoint.access.can_write() {
        revents |= pipe_tx_revents(pipe, interests);
    }
    revents
}

pub(super) fn pipe_poll(
    file: &File,
    request: &PollRequest<'_>,
) -> Result<PollRegisterResult, SysError> {
    let endpoint = file
        .prv()
        .cast::<PipeEndpoint>()
        .expect("internal error: pipe file without endpoint private data");
    let mut pipe = endpoint.pipe.inner.lock();
    if !request.is_register() {
        return Ok(PollRegisterResult::Ready(pipe_revents(
            &pipe,
            endpoint,
            request.interests(),
        )));
    }
    let Some(route) = request.route() else {
        let revents = pipe_revents(&pipe, endpoint, request.interests());
        return Ok(if revents.is_empty() {
            PollRegisterResult::Unsupported
        } else {
            PollRegisterResult::Ready(revents)
        });
    };
    // Prepare every needed replacement before publishing either one, so a
    // duplex endpoint cannot leave a half-subscribed route after ENOMEM.
    let rx_prepared = endpoint
        .access
        .can_read()
        .then(|| prepare_pipe_poll_routes(&pipe.rx_poll_routes, route, request.interests()))
        .transpose()?;
    let tx_prepared = endpoint
        .access
        .can_write()
        .then(|| prepare_pipe_poll_routes(&pipe.tx_poll_routes, route, request.interests()))
        .transpose()?;
    let mut previous_rx = None;
    let mut previous_tx = None;
    let mut pruned = 0usize;
    if let Some((replacement, removed)) = rx_prepared {
        previous_rx = Some(core::mem::replace(&mut pipe.rx_poll_routes, replacement));
        pruned += removed;
    }
    if let Some((replacement, removed)) = tx_prepared {
        previous_tx = Some(core::mem::replace(&mut pipe.tx_poll_routes, replacement));
        pruned += removed;
    }
    let revents = pipe_revents(&pipe, endpoint, request.interests());
    let rx_queue_len = pipe.rx_poll_routes.len();
    let tx_queue_len = pipe.tx_poll_routes.len();
    drop(pipe);
    drop(previous_rx);
    drop(previous_tx);
    kdebugln!(
        "pipe: subscribed poll interests={:?} rx_queue_len={} tx_queue_len={} pruned={}",
        request.interests(),
        rx_queue_len,
        tx_queue_len,
        pruned,
    );
    Ok(PollRegisterResult::Subscribed(revents))
}
