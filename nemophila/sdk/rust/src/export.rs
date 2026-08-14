pub use crate::__bindings::Guest;

pub fn load<M: crate::Module>() -> Result<(), ()> {
    let mut context = crate::LoadContext {
        _load: core::marker::PhantomData,
    };
    // The Host only observes whether the module accepted the load. A
    // module-local error remains guest input and is not a second wire-level
    // statement about runtime binding state.
    M::load(&mut context).map_err(|_| ())
}

pub fn observe_clone(creator_tid: u32, child_tid: u32) {
    crate::weave::task::clone_observer::invoke(crate::weave::task::clone_observer::CloneEvent {
        creator_tid,
        child_tid,
    });
}
