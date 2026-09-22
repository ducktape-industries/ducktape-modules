// harness: runs the forge program on the ducktape runtime over MemoryHost and serves it over HTTP; used by forge-harness and the git tests.

mod host;
mod server;

use std::sync::Arc;

use abi::{GuestCall, HashKind, Invocation, Refusal};
use forge::{Bounds, Op, Query};
use runtime::{Code, Fault, Limits, Runtime};
use tokio::sync::Mutex;

pub use host::MemoryHost;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    Refused(Refusal),
    Faulted(Fault),
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Failure::Refused(refusal) => write!(f, "{refusal}"),
            Failure::Faulted(fault) => write!(f, "{fault}"),
        }
    }
}

impl std::error::Error for Failure {}

impl From<Fault> for Failure {
    fn from(fault: Fault) -> Failure {
        Failure::Faulted(fault)
    }
}

impl From<Refusal> for Failure {
    fn from(refusal: Refusal) -> Failure {
        Failure::Refused(refusal)
    }
}

pub struct Program {
    runtime: Runtime,
    code: Code,
    host: MemoryHost,
}

impl Program {
    async fn run(&mut self, call: GuestCall) -> Result<(), Failure> {
        let invocation = Invocation {
            env: self.host.env(),
            call,
        };
        self.runtime
            .run(&self.code, invocation, &mut self.host)
            .await??;
        Ok(())
    }

    pub async fn execute(&mut self, op: &Op) -> Result<Vec<u8>, Failure> {
        self.host.advance_height();
        for delivered in self.host.deliver() {
            delivered?;
        }
        let before = self.host.clone();
        let ran = self.run(GuestCall::Execute(abi::encode(op))).await;
        let output = self.host.take_output();
        if ran.is_err() {
            self.host = before;
        }
        ran.map(|()| output)
    }

    pub async fn query(&mut self, query: &Query) -> Result<Vec<u8>, Failure> {
        let ran = self.run(GuestCall::Query(abi::encode(query))).await;
        let response = self.host.take_response();
        ran.map(|()| response)
    }
}

#[derive(Clone)]
pub struct Harness {
    program: Arc<Mutex<Program>>,
}

impl Harness {
    pub async fn new(wasm: &[u8], bounds: Bounds, actor: Vec<u8>) -> Result<Harness, Failure> {
        let runtime = Runtime::new(Limits {
            fuel: None,
            memory_bytes: None,
        });
        let code = runtime.load(wasm)?;
        let mut program = Program {
            runtime,
            code,
            host: MemoryHost::new(actor),
        };
        program.run(GuestCall::Init(abi::encode(&bounds))).await?;
        Ok(Harness {
            program: Arc::new(Mutex::new(program)),
        })
    }

    pub fn from_snapshot(wasm: &[u8], host: MemoryHost) -> Result<Self, Failure> {
        let runtime = Runtime::new(Limits {
            fuel: None,
            memory_bytes: None,
        });
        let code = runtime.load(wasm)?;
        Ok(Self {
            program: Arc::new(Mutex::new(Program {
                runtime,
                code,
                host,
            })),
        })
    }
    pub async fn snapshot(&self) -> MemoryHost {
        self.program.lock().await.host.clone()
    }
    pub async fn set_actor(&self, actor: Vec<u8>) {
        self.program.lock().await.host.set_actor(actor);
    }
    pub async fn advance(&self) -> Result<(), Failure> {
        let mut p = self.program.lock().await;
        p.host.advance_height();
        for outcome in p.host.deliver() {
            outcome?;
        }
        Ok(())
    }
    pub async fn chat_execute(
        &self,
        party: chat::Party,
        msg: chat::ChatMsg,
    ) -> Result<(), Failure> {
        self.advance().await?;
        self.program.lock().await.host.chat_execute(party, msg)?;
        Ok(())
    }
    pub async fn chat_query(&self, q: chat::ChatViewQuery) -> Result<chat::ChatViewReply, Failure> {
        Ok(self.program.lock().await.host.chat_query(q)?)
    }

    pub async fn create_repo(&self, name: &str, hash: HashKind) -> Result<(), Failure> {
        self.execute(&Op::Create {
            repo: name.into(),
            hash,
        })
        .await
        .map(|_| ())
    }

    pub async fn execute(&self, op: &Op) -> Result<Vec<u8>, Failure> {
        self.program.lock().await.execute(op).await
    }

    pub async fn query(&self, query: &Query) -> Result<Vec<u8>, Failure> {
        self.program.lock().await.query(query).await
    }

    pub fn serve(
        &self,
        listener: tokio::net::TcpListener,
    ) -> impl std::future::Future<Output = std::io::Result<()>> + Send + 'static {
        axum::serve(listener, server::router(self.clone())).into_future()
    }
}

pub fn default_bounds() -> Bounds {
    Bounds {
        max_objects: 1_000_000,
        max_delta_depth: 64,
        max_object_size: 256 << 20,
        push_walk: 10_000,
        fetch_walk: 1_000_000,
        merge_cost: 1024,
        page_size: 128,
        log_walk: 10_000,
        tree_walk: 1024,
        diff_bytes: 32 << 20,
        blob_bytes: 256 << 10,
        record_bytes: 64 << 10,
    }
}
