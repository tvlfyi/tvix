use std::io::{Error, ErrorKind};

use enum_primitive_derive::Primitive;
use num_traits::{FromPrimitive, ToPrimitive};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::wire::primitive;

/// Worker Operation
///
/// These operations are encoded as unsigned 64 bits before being sent
/// to the wire. See the [read_op] and
/// [write_op] operations to serialize/deserialize the
/// operation on the wire.
///
/// Note: for now, we're using the Nix 2.20 operation description. The
/// operations marked as obsolete are obsolete for Nix 2.20, not
/// necessarily for Nix 2.3. We'll revisit this later on.
#[derive(Debug, PartialEq, Primitive)]
pub enum Operation {
    IsValidPath = 1,
    HasSubstitutes = 3,
    QueryPathHash = 4,   // obsolete
    QueryReferences = 5, // obsolete
    QueryReferrers = 6,
    AddToStore = 7,
    AddTextToStore = 8, // obsolete since 1.25, Nix 3.0. Use WorkerProto::Op::AddToStore
    BuildPaths = 9,
    EnsurePath = 10,
    AddTempRoot = 11,
    AddIndirectRoot = 12,
    SyncWithGC = 13,
    FindRoots = 14,
    ExportPath = 16,   // obsolete
    QueryDeriver = 18, // obsolete
    SetOptions = 19,
    CollectGarbage = 20,
    QuerySubstitutablePathInfo = 21,
    QueryDerivationOutputs = 22, // obsolete
    QueryAllValidPaths = 23,
    QueryFailedPaths = 24,
    ClearFailedPaths = 25,
    QueryPathInfo = 26,
    ImportPaths = 27,                // obsolete
    QueryDerivationOutputNames = 28, // obsolete
    QueryPathFromHashPart = 29,
    QuerySubstitutablePathInfos = 30,
    QueryValidPaths = 31,
    QuerySubstitutablePaths = 32,
    QueryValidDerivers = 33,
    OptimiseStore = 34,
    VerifyStore = 35,
    BuildDerivation = 36,
    AddSignatures = 37,
    NarFromPath = 38,
    AddToStoreNar = 39,
    QueryMissing = 40,
    QueryDerivationOutputMap = 41,
    RegisterDrvOutput = 42,
    QueryRealisation = 43,
    AddMultipleToStore = 44,
    AddBuildLog = 45,
    BuildPathsWithResults = 46,
    AddPermRoot = 47,
}

/// Read a worker [Operation] from the wire.
pub async fn read_op<R: AsyncReadExt + Unpin>(r: &mut R) -> std::io::Result<Operation> {
    let op_number = primitive::read_u64(r).await?;
    Operation::from_u64(op_number).ok_or(Error::new(
        ErrorKind::Other,
        format!("Invalid OP number {}", op_number),
    ))
}

/// Write a worker [Operation] to the wire.
pub async fn write_op<W: AsyncWriteExt + Unpin>(w: &mut W, op: &Operation) -> std::io::Result<()> {
    let op = Operation::to_u64(op).ok_or(Error::new(
        ErrorKind::Other,
        format!("Can't convert the OP {:?} to u64", op),
    ))?;
    w.write_u64(op).await
}

#[derive(Debug, PartialEq)]
pub enum Trust {
    Trusted,
    NotTrusted,
}

/// Write the worker [Trust] level to the wire.
///
/// Cpp Nix has a legacy third option: u8 0. This option is meant to
/// be used as a backward compatible measure. Since we're not
/// targetting protocol versions pre-dating the trust notion, we
/// decided not to implement it here.
pub async fn write_worker_trust_level<W>(conn: &mut W, t: Trust) -> std::io::Result<()>
where
    W: AsyncReadExt + AsyncWriteExt + Unpin + std::fmt::Debug,
{
    match t {
        Trust::Trusted => primitive::write_u64(conn, 1).await,
        Trust::NotTrusted => primitive::write_u64(conn, 2).await,
    }
}
