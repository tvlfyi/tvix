//! The composition module allows composing different kinds of services based on a set of service
//! configurations _at runtime_.
//!
//! Store configs are deserialized with serde. The registry provides a stateful mapping from the
//! `type` tag of an internally tagged enum on the serde side to a Config struct which is
//! deserialized and then returned as a `Box<dyn ServiceBuilder<Output = dyn BlobService>>`
//! (the same for DirectoryService instead of BlobService etc).
//!
//! ### Example 1.: Implementing a new BlobService
//!
//! You need a Config struct which implements `DeserializeOwned` and
//! `ServiceBuilder<Output = dyn BlobService>`.
//! Provide the user with a function to call with
//! their registry. You register your new type as:
//!
//! ```
//! use std::sync::Arc;
//!
//! use tvix_castore::composition::*;
//! use tvix_castore::blobservice::BlobService;
//!
//! #[derive(serde::Deserialize)]
//! struct MyBlobServiceConfig {
//! }
//!
//! #[tonic::async_trait]
//! impl ServiceBuilder for MyBlobServiceConfig {
//!     type Output = dyn BlobService;
//!     async fn build(&self, _: &str, _: &CompositionContext<Self::Output>) -> Result<Arc<Self::Output>, Box<dyn std::error::Error + Send + Sync + 'static>> {
//!         todo!()
//!     }
//! }
//!
//! impl TryFrom<url::Url> for MyBlobServiceConfig {
//!     type Error = Box<dyn std::error::Error + Send + Sync>;
//!     fn try_from(url: url::Url) -> Result<Self, Self::Error> {
//!         todo!()
//!     }
//! }
//!
//! pub fn add_my_service(reg: &mut Registry) {
//!     reg.register::<Box<dyn ServiceBuilder<Output = dyn BlobService>>, MyBlobServiceConfig>("myblobservicetype");
//! }
//! ```
//!
//! Now, when a user deserializes a store config with the type tag "myblobservicetype" into a
//! `Box<dyn ServiceBuilder<Output = Arc<dyn BlobService>>>`, it will be done via `MyBlobServiceConfig`.
//!
//! ### Example 2.: Composing stores to get one store
//!
//! ```
//! use tvix_castore::composition::*;
//! use tvix_castore::blobservice::BlobService;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! # tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async move {
//! let blob_services_configs_json = serde_json::json!({
//!   "blobstore1": {
//!     "type": "memory"
//!   },
//!   "blobstore2": {
//!     "type": "memory"
//!   },
//!   "default": {
//!     "type": "combined",
//!     "local": "blobstore1",
//!     "remote": "blobstore2"
//!   }
//! });
//!
//! let blob_services_configs = with_registry(&REG, || serde_json::from_value(blob_services_configs_json))?;
//! let blob_service_composition = Composition::<dyn BlobService>::from_configs(blob_services_configs);
//! let blob_service = blob_service_composition.build("default").await?;
//! # Ok(())
//! # })
//! # }
//! ```
//!
//! ### Example 3.: Creating another registry extending the default registry with third-party types
//!
//! ```
//! # pub fn add_my_service(reg: &mut tvix_castore::composition::Registry) {}
//! let mut my_registry = tvix_castore::composition::Registry::default();
//! tvix_castore::composition::add_default_services(&mut my_registry);
//! add_my_service(&mut my_registry);
//! ```
//!
//! Continue with Example 2, with my_registry instead of REG

use erased_serde::deserialize;
use futures::future::BoxFuture;
use futures::FutureExt;
use lazy_static::lazy_static;
use serde::de::DeserializeOwned;
use serde_tagged::de::{BoxFnSeed, SeedFactory};
use serde_tagged::util::TagString;
use std::any::{Any, TypeId};
use std::cell::Cell;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::Arc;
use tonic::async_trait;

/// Resolves tag names to the corresponding Config type.
// Registry implementation details:
// This is really ugly. Really we would want to store this as a generic static field:
//
// ```
// struct Registry<T>(BTreeMap<(&'static str), RegistryEntry<T>);
// static REG<T>: Registry<T>;
// ```
//
// so that one version of the static is generated for each Type that the registry is accessed for.
// However, this is not possible, because generics are only a thing in functions, and even there
// they will not interact with static items:
// https://doc.rust-lang.org/reference/items/static-items.html#statics--generics
//
// So instead, we make this lookup at runtime by putting the TypeId into the key.
// But now we can no longer store the `BoxFnSeed<T>` because we are lacking the generic parameter
// T, so instead store it as `Box<dyn Any>` and downcast to `&BoxFnSeed<T>` when performing the
// lookup.
// I said it was ugly...
#[derive(Default)]
pub struct Registry(BTreeMap<(TypeId, &'static str), Box<dyn Any + Sync>>);
pub type FromUrlSeed<T> =
    Box<dyn Fn(url::Url) -> Result<T, Box<dyn std::error::Error + Send + Sync>> + Sync>;
pub struct RegistryEntry<T> {
    serde_deserialize_seed: BoxFnSeed<DeserializeWithRegistry<T>>,
    from_url_seed: FromUrlSeed<DeserializeWithRegistry<T>>,
}

struct RegistryWithFakeType<'r, T>(&'r Registry, PhantomData<T>);

impl<'r, 'de: 'r, T: 'static> SeedFactory<'de, TagString<'de>> for RegistryWithFakeType<'r, T> {
    type Value = DeserializeWithRegistry<T>;
    type Seed = &'r BoxFnSeed<Self::Value>;

    // Required method
    fn seed<E>(self, tag: TagString<'de>) -> Result<Self::Seed, E>
    where
        E: serde::de::Error,
    {
        // using find() and not get() because of https://github.com/rust-lang/rust/issues/80389
        let seed: &Box<dyn Any + Sync> = self
            .0
             .0
            .iter()
            .find(|(k, _)| *k == &(TypeId::of::<T>(), tag.as_ref()))
            .ok_or_else(|| serde::de::Error::custom("Unknown tag"))?
            .1;

        let entry: &RegistryEntry<T> = <dyn Any>::downcast_ref(&**seed).unwrap();

        Ok(&entry.serde_deserialize_seed)
    }
}

/// Wrapper type which implements Deserialize using the registry
///
/// Wrap your type in this in order to deserialize it using a registry, e.g.
/// `RegistryWithFakeType<Box<dyn MyTrait>>`, then the types registered for `Box<dyn MyTrait>`
/// will be used.
pub struct DeserializeWithRegistry<T>(pub T);

impl Registry {
    /// Registers a mapping from type tag to a concrete type into the registry.
    ///
    /// The type parameters are very important:
    /// After calling `register::<Box<dyn FooTrait>, FooStruct>("footype")`, when a user
    /// deserializes into an input with the type tag "myblobservicetype" into a
    /// `Box<dyn FooTrait>`, it will first call the Deserialize imple of `FooStruct` and
    /// then convert it into a `Box<dyn FooTrait>` using From::from.
    pub fn register<
        T: 'static,
        C: DeserializeOwned
            + TryFrom<url::Url, Error = Box<dyn std::error::Error + Send + Sync>>
            + Into<T>,
    >(
        &mut self,
        type_name: &'static str,
    ) {
        self.0.insert(
            (TypeId::of::<T>(), type_name),
            Box::new(RegistryEntry {
                serde_deserialize_seed: BoxFnSeed::new(|x| {
                    deserialize::<C>(x)
                        .map(Into::into)
                        .map(DeserializeWithRegistry)
                }),
                from_url_seed: Box::new(|url| {
                    C::try_from(url)
                        .map(Into::into)
                        .map(DeserializeWithRegistry)
                }),
            }),
        );
    }
}

impl<'de, T: 'static> serde::Deserialize<'de> for DeserializeWithRegistry<T> {
    fn deserialize<D>(de: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        serde_tagged::de::internal::deserialize(
            de,
            "type",
            RegistryWithFakeType(ACTIVE_REG.get().unwrap(), PhantomData::<T>),
        )
    }
}

#[derive(Debug, thiserror::Error)]
enum TryFromUrlError {
    #[error("Unknown tag: {0}")]
    UnknownTag(String),
}

impl<T: 'static> TryFrom<url::Url> for DeserializeWithRegistry<T> {
    type Error = Box<dyn std::error::Error + Send + Sync>;
    fn try_from(url: url::Url) -> Result<Self, Self::Error> {
        let tag = url.scheme().split('+').next().unwrap();
        // same as in the SeedFactory impl: using find() and not get() because of https://github.com/rust-lang/rust/issues/80389
        let seed = ACTIVE_REG
            .get()
            .unwrap()
            .0
            .iter()
            .find(|(k, _)| *k == &(TypeId::of::<T>(), tag))
            .ok_or_else(|| Box::new(TryFromUrlError::UnknownTag(tag.into())))?
            .1;
        let entry: &RegistryEntry<T> = <dyn Any>::downcast_ref(&**seed).unwrap();
        (entry.from_url_seed)(url)
    }
}

thread_local! {
    /// The active Registry is global state, because there is no convenient and universal way to pass state
    /// into the functions usually used for deserialization, e.g. `serde_json::from_str`, `toml::from_str`,
    /// `serde_qs::from_str`.
    static ACTIVE_REG: Cell<Option<&'static Registry>> = panic!("reg was accessed before initialization");
}

/// Run the provided closure with a registry context.
/// Any serde deserialize calls within the closure will use the registry to resolve tag names to
/// the corresponding Config type.
pub fn with_registry<R>(reg: &'static Registry, f: impl FnOnce() -> R) -> R {
    ACTIVE_REG.set(Some(reg));
    let result = f();
    ACTIVE_REG.set(None);
    result
}

lazy_static! {
    /// The provided registry of tvix_castore, with all builtin BlobStore/DirectoryStore implementations
    pub static ref REG: Registry = {
        let mut reg = Default::default();
        add_default_services(&mut reg);
        reg
    };
}

// ---------- End of generic registry code --------- //

/// Register the builtin services of tvix_castore with the given registry.
/// This is useful for creating your own registry with the builtin types _and_
/// extra third party types.
pub fn add_default_services(reg: &mut Registry) {
    crate::blobservice::register_blob_services(reg);
    crate::directoryservice::register_directory_services(reg);
}

pub struct CompositionContext<'a, T: ?Sized> {
    stack: Vec<String>,
    composition: Option<&'a Composition<T>>,
}

impl<'a, T: ?Sized + Send + Sync + 'static> CompositionContext<'a, T> {
    pub fn blank() -> Self {
        Self {
            stack: Default::default(),
            composition: None,
        }
    }

    pub async fn resolve(
        &self,
        entrypoint: String,
    ) -> Result<Arc<T>, Box<dyn std::error::Error + Send + Sync + 'static>> {
        // disallow recursion
        if self.stack.contains(&entrypoint) {
            return Err(CompositionError::Recursion(self.stack.clone()).into());
        }
        match self.composition {
            Some(comp) => Ok(comp.build_internal(self.stack.clone(), entrypoint).await?),
            None => Err(CompositionError::NotFound(entrypoint).into()),
        }
    }
}

#[async_trait]
/// This is the trait usually implemented on a per-store-type Config struct and
/// used to instantiate it.
pub trait ServiceBuilder: Send + Sync {
    type Output: ?Sized;
    async fn build(
        &self,
        instance_name: &str,
        context: &CompositionContext<Self::Output>,
    ) -> Result<Arc<Self::Output>, Box<dyn std::error::Error + Send + Sync + 'static>>;
}

impl<T: ?Sized, S: ServiceBuilder<Output = T> + 'static> From<S>
    for Box<dyn ServiceBuilder<Output = T>>
{
    fn from(t: S) -> Self {
        Box::new(t)
    }
}

enum InstantiationState<T: ?Sized> {
    Config(Box<dyn ServiceBuilder<Output = T>>),
    InProgress(tokio::sync::watch::Receiver<Option<Result<Arc<T>, CompositionError>>>),
    Done(Result<Arc<T>, CompositionError>),
}

pub struct Composition<T: ?Sized> {
    stores: std::sync::Mutex<HashMap<String, InstantiationState<T>>>,
}

#[derive(thiserror::Error, Clone, Debug)]
pub enum CompositionError {
    #[error("store not found: {0}")]
    NotFound(String),
    #[error("recursion not allowed {0:?}")]
    Recursion(Vec<String>),
    #[error("store construction panicked {0}")]
    Poisoned(String),
    #[error("instantiation of service {0} failed: {1}")]
    Failed(String, Arc<dyn std::error::Error + Send + Sync>),
}

impl<T: ?Sized + Send + Sync + 'static> Composition<T> {
    pub fn from_configs(
        // Keep the concrete `HashMap` type here since it allows for type
        // inference of what type is previously being deserialized.
        configs: HashMap<String, DeserializeWithRegistry<Box<dyn ServiceBuilder<Output = T>>>>,
    ) -> Self {
        Self::from_configs_iter(configs)
    }

    pub fn from_configs_iter(
        configs: impl IntoIterator<
            Item = (
                String,
                DeserializeWithRegistry<Box<dyn ServiceBuilder<Output = T>>>,
            ),
        >,
    ) -> Self {
        Composition {
            stores: std::sync::Mutex::new(
                configs
                    .into_iter()
                    .map(|(k, v)| (k, InstantiationState::Config(v.0)))
                    .collect(),
            ),
        }
    }

    pub async fn build(&self, entrypoint: &str) -> Result<Arc<T>, CompositionError> {
        self.build_internal(vec![], entrypoint.to_string()).await
    }

    fn build_internal(
        &self,
        stack: Vec<String>,
        entrypoint: String,
    ) -> BoxFuture<'_, Result<Arc<T>, CompositionError>> {
        let mut stores = self.stores.lock().unwrap();
        let entry = match stores.get_mut(&entrypoint) {
            Some(v) => v,
            None => return Box::pin(futures::future::err(CompositionError::NotFound(entrypoint))),
        };
        // for lifetime reasons, we put a placeholder value in the hashmap while we figure out what
        // the new value should be. the Mutex stays locked the entire time, so nobody will ever see
        // this temporary value.
        let prev_val = std::mem::replace(
            entry,
            InstantiationState::Done(Err(CompositionError::Poisoned(entrypoint.clone()))),
        );
        let (new_val, ret) = match prev_val {
            InstantiationState::Done(service) => (
                InstantiationState::Done(service.clone()),
                futures::future::ready(service).boxed(),
            ),
            // the construction of the store has not started yet.
            InstantiationState::Config(config) => {
                let (tx, rx) = tokio::sync::watch::channel(None);
                (
                    InstantiationState::InProgress(rx),
                    (async move {
                        let mut new_context = CompositionContext {
                            stack: stack.clone(),
                            composition: Some(self),
                        };
                        new_context.stack.push(entrypoint.clone());
                        let res = config
                            .build(&entrypoint, &new_context)
                            .await
                            .map_err(|e| CompositionError::Failed(entrypoint, e.into()));
                        tx.send(Some(res.clone())).unwrap();
                        res
                    })
                    .boxed(),
                )
            }
            // there is already a task driving forward the construction of this store, wait for it
            // to notify us via the provided channel
            InstantiationState::InProgress(mut recv) => {
                (InstantiationState::InProgress(recv.clone()), {
                    (async move {
                        loop {
                            if let Some(v) =
                                recv.borrow_and_update().as_ref().map(|res| res.clone())
                            {
                                break v;
                            }
                            recv.changed().await.unwrap();
                        }
                    })
                    .boxed()
                })
            }
        };
        *entry = new_val;
        ret
    }
}
