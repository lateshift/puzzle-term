//! Typed adapters for plugin hooks at the only `Any` seam (design §5.4).
//!
//! A generic wrapper checks `TypeId` before downcasting, so plugin callback
//! code receives its concrete message type. The wrapper refreshes the wire
//! view after a command rewrite, preserving what a later recorder sees.
use super::Bus;
use crate::{
    hooks::{CommandKinds, EventKinds, Flow, HookCtx, HookId, Interceptor, Observer},
    message::Message,
    wire::WireExt,
};
use std::{any::TypeId, marker::PhantomData};

/// Ties typed adapters to both plugin and domain types without storing any.
type AdapterMarker<T, C, E> = PhantomData<fn() -> (T, C, E)>;

/// Generic observer closure and its concrete plugin type.
struct TypedObserver<T, F, C: Message, E: Message> {
    /// Callback sees the decoded plugin message.
    callback: F,
    /// Type identity is captured without storing a value.
    marker: AdapterMarker<T, C, E>,
}

/// Safely adapts a typed callback to the generic observer table.
impl<T, F, C, E> Observer<C, E> for TypedObserver<T, F, C, E>
where
    T: WireExt,
    C: Message,
    E: Message,
    F: FnMut(&T, &mut HookCtx<'_, C, E>) + Send,
{
    /// Ignores extensions of another registered concrete type.
    fn observe(&mut self, event: &E, ctx: &mut HookCtx<'_, C, E>) {
        // The shared extension bit admits every plugin. The concrete type
        // check prevents one plugin's observer seeing another plugin's data.
        // An enum owned by any domain may expose its plugin seam through
        // `Message`; the adapter never matches a terminal-specific variant.
        if let Some(extension) = event.extension()
            && let Some(value) = extension.downcast_ref::<T>()
        {
            // Observers borrow the value, since a fact cannot be vetoed or
            // rewritten after it has already happened (§5.1).
            (self.callback)(value, ctx);
        }
    }
}

/// Generic interceptor closure and its concrete plugin type.
struct TypedInterceptor<T, F, C: Message, E: Message> {
    /// Callback may rewrite or veto the typed message.
    callback: F,
    /// Type identity is captured without storing a value.
    marker: AdapterMarker<T, C, E>,
}

/// Safely adapts a typed policy to the generic interceptor table.
impl<T, F, C, E> Interceptor<C, E> for TypedInterceptor<T, F, C, E>
where
    T: WireExt,
    C: Message,
    E: Message,
    F: FnMut(&mut T, &mut HookCtx<'_, C, E>) -> Flow + Send,
{
    /// Refreshes the public value after a successful typed rewrite.
    fn intercept(&mut self, command: &mut C, ctx: &mut HookCtx<'_, C, E>) -> Flow {
        // Only this plugin's command may be passed to its typed callback.
        // Borrow the concrete plugin only after the domain confirms this
        // command is its extension variant; other requests bypass downcast.
        if let Some(extension) = command.extension_mut()
            && let Some(value) = extension.downcast_mut::<T>()
        {
            let flow = (self.callback)(value, ctx);
            // A recorder reads the wire view after interception. Refresh it
            // from the rewritten typed value before the handler can execute.
            if extension.refresh::<T>().is_err() {
                // An invalid rewrite must not reach the terminal handler.
                return Flow::Stop("extension rewrite produced invalid wire data");
            }
            // Propagate the plugin's stop decision to the regular policy
            // chain; later interceptors must not run after a veto.
            return flow;
        }
        Flow::Continue
    }
}

/// Public typed registration methods hide the downcast implementation.
impl<C: Message, E: Message> Bus<C, E> {
    /// Observes one concrete extension event at a chosen priority.
    pub fn observe_ext<T: WireExt>(
        &mut self,
        priority: i16,
        name: &'static str,
        callback: impl FnMut(&T, &mut HookCtx<'_, C, E>) + Send + 'static,
    ) -> Result<HookId, &'static str> {
        // All extension events share one mask bit; the adapter narrows by
        // TypeId inside the existing priority-ordered observer sequence.
        let id = self.observe(
            EventKinds::at(E::EXT_INDEX),
            priority,
            name,
            TypedObserver::<T, _, C, E> {
                callback,
                marker: PhantomData,
            },
        )?;
        // Index concrete listeners so a different plugin's publication
        // stays on the zero-allocation path before Extension::new runs.
        if let Some(slot) = self.observers.iter_mut().find(|slot| slot.id == id) {
            slot.extension = Some(TypeId::of::<T>());
        }
        self.recompute_observed();
        Ok(id)
    }

    /// Intercepts one concrete extension command before execution.
    pub fn intercept_ext<T: WireExt>(
        &mut self,
        priority: i16,
        name: &'static str,
        callback: impl FnMut(&mut T, &mut HookCtx<'_, C, E>) -> Flow + Send + 'static,
    ) -> Result<HookId, &'static str> {
        // Reuse the ordinary policy queue so plugin commands retain origin,
        // veto and cascade semantics identical to built-in commands.
        let id = self.intercept(
            CommandKinds::at(C::EXT_INDEX),
            priority,
            name,
            TypedInterceptor::<T, _, C, E> {
                callback,
                marker: PhantomData,
            },
        )?;
        // The drain can skip an unrelated concrete plugin before calling
        // the generic adapter's downcast.
        if let Some(slot) = self.interceptors.iter_mut().find(|slot| slot.id == id) {
            slot.extension = Some(TypeId::of::<T>());
        }
        Ok(id)
    }
}
