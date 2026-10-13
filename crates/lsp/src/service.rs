use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::Result;
use jsonrpc::{JsonRpcService, RequestId, ServerNotificationEvent};
use lsp_types::notification::{self, Notification};
use lsp_types::request::{self, Request};
use lsp_types::{
    CancelParams, DidChangeTextDocumentParams, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, DocumentFormattingParams, FormattingOptions, GotoDefinitionParams,
    GotoDefinitionResponse, HoverParams, InitializeParams, InitializedParams, NumberOrString,
    Position, ReferenceParams, TextDocumentIdentifier, TextDocumentItem,
    TextDocumentPositionParams, VersionedTextDocumentIdentifier,
};
use serde_json::Value;
use simple_logger::SimpleLogger;
use warp_errors::report_error;
use warp_util::on_cancel::OnCancelFutureExt;

use crate::LspServerLogLevel;
use crate::config::{LanguageId, path_to_lsp_uri};
use crate::types::{
    HoverResult, LspDefinitionLocation, ReferenceLocation, TextDocumentContentChangeEvent, TextEdit,
};

/// Tracks the sync state for an open document.
#[derive(Debug, Clone)]
pub struct DocumentSyncState {
    /// The last buffer version that was successfully synced with the LSP server.
    /// None means the document was opened but no subsequent changes have been synced yet.
    pub last_synced_version: Option<usize>,
}

pub struct LspService {
    jsonrpc_service: JsonRpcService,
    server_capabilities: Option<lsp_types::ServerCapabilities>,
    open_documents: Arc<Mutex<HashMap<PathBuf, DocumentSyncState>>>,
    notify_tx: async_channel::Sender<ServerNotificationEvent>,
    logger: Option<SimpleLogger>,
}

impl LspService {
    /// Creates a new LspService with the given JsonRpcService.
    pub(crate) fn new(
        jsonrpc_service: JsonRpcService,
        notify_tx: async_channel::Sender<ServerNotificationEvent>,
        logger: Option<SimpleLogger>,
    ) -> Result<Self> {
        let service = Self {
            jsonrpc_service,
            server_capabilities: None,
            open_documents: Arc::new(Mutex::new(HashMap::new())),
            notify_tx,
            logger,
        };

        Ok(service)
    }

    pub(crate) async fn initialize(&mut self, params: InitializeParams) -> Result<()> {
        let response = self.send_request::<request::Initialize>(params).await?;
        self.server_capabilities = Some(response.capabilities);

        self.send_notification::<notification::Initialized>(InitializedParams {})?;
        self.subscribe::<notification::Progress>().await;
        self.subscribe::<notification::PublishDiagnostics>().await;

        log::info!("LSP initialized successfully and will now run startup tasks");
        Ok(())
    }

    pub(crate) async fn subscribe<N: notification::Notification>(&self) {
        self.jsonrpc_service
            .subscribe(N::METHOD.to_string(), self.notify_tx.clone())
            .await;
    }

    pub fn server_capabilities(&self) -> &Option<lsp_types::ServerCapabilities> {
        &self.server_capabilities
    }

    pub fn log_to_server_log(&self, level: LspServerLogLevel, message: impl Into<String>) {
        if let Some(logger) = &self.logger {
            logger.log(format!("[{level}] {}", message.into()));
        }
    }

    pub async fn shutdown(&self) -> anyhow::Result<()> {
        // Send LSP shutdown request first
        log::debug!("Sending LSP shutdown request");
        match self.send_request::<request::Shutdown>(()).await {
            Ok(_) => log::debug!("LSP shutdown request completed successfully"),
            Err(e) => {
                log::warn!("LSP shutdown request failed: {e}");
                // Continue with exit notification even if shutdown request fails
            }
        }

        // Send exit notification
        log::debug!("Sending LSP exit notification");
        if let Err(e) = self.send_notification::<notification::Exit>(()) {
            log::warn!("Failed to send LSP exit notification: {e}");
        }

        // Finally, shutdown the transport (kill process if needed)
        self.jsonrpc_service
            .shutdown(std::time::Duration::from_secs(5))
            .await?;

        log::debug!("LSP shutdown sequence completed");
        Ok(())
    }

    /// Get a handle to text-document related operations and requests
    pub fn text_document(&self) -> TextDocumentService<'_> {
        TextDocumentService {
            service: self,
            open_documents: self.open_documents.clone(),
        }
    }

    async fn send_request_internal(&self, method: String, params: Value) -> Result<Value> {
        let request_id = self.jsonrpc_service.next_id();

        let request = self
            .jsonrpc_service
            .send_request(request_id, method, params);

        request
            .on_cancel(move || {
                self.cancel_request(request_id);
            })
            .await
    }

    fn send_notification<N: Notification>(&self, params: N::Params) -> Result<()> {
        let params = serde_json::to_value(params)?;
        self.jsonrpc_service
            .send_notification(N::METHOD.to_string(), params)
    }

    async fn send_request<R: Request>(&self, params: R::Params) -> Result<R::Result> {
        let params = serde_json::to_value(params)?;
        let request = self.send_request_internal(R::METHOD.to_string(), params);
        let response = request.await?;
        serde_json::from_value::<R::Result>(response)
            .map_err(|e| anyhow::anyhow!("Failed to parse response: {e}"))
    }

    fn cancel_request(&self, request_id: RequestId) {
        let cancel = serde_json::to_value(CancelParams {
            id: NumberOrString::Number(request_id),
        })
        .expect("Failed to serialize cancel params");

        if let Err(e) = self
            .jsonrpc_service
            .send_notification(notification::Cancel::METHOD.to_string(), cancel)
        {
            report_error!(e.context("Failed to send cancel notification"));
        }
    }
}

/// Encapsulates text-document related operations and requests. This exists only for bookkeeping
/// and discoverability.
pub struct TextDocumentService<'a> {
    service: &'a LspService,
    open_documents: Arc<Mutex<HashMap<PathBuf, DocumentSyncState>>>,
}

impl<'a> TextDocumentService<'a> {
    pub fn document_is_open(&self, path: &PathBuf) -> Result<bool> {
        let open_documents = self
            .open_documents
            .lock()
            .map_err(|_| anyhow::anyhow!("Failed to acquire lock on open documents"))?;
        Ok(open_documents.contains_key(path))
    }

    /// Returns the last synced buffer version for the document, if it is open.
    /// Returns None if the document is not open.
    pub fn last_synced_version(&self, path: &PathBuf) -> Result<Option<usize>> {
        let open_documents = self
            .open_documents
            .lock()
            .map_err(|_| anyhow::anyhow!("Failed to acquire lock on open documents"))?;
        Ok(open_documents
            .get(path)
            .and_then(|state| state.last_synced_version))
    }

    pub async fn did_open(
        &self,
        path: &Path,
        content: String,
        initial_version: usize,
    ) -> Result<()> {
        {
            // Drop the guard before await
            let mut open_documents = self
                .open_documents
                .lock()
                .map_err(|_| anyhow::anyhow!("Failed to acquire lock on open documents"))?;
            // Use entry API to check if already present
            if open_documents.contains_key(&path.to_path_buf()) {
                return Ok(());
            }
            open_documents.insert(
                path.to_path_buf(),
                DocumentSyncState {
                    last_synced_version: Some(initial_version),
                },
            );
        }

        self.service.log_to_server_log(
            LspServerLogLevel::Debug,
            format!(
                "didOpen -> server: file={} version={initial_version}",
                path.display()
            ),
        );

        // Determine language ID from the file path
        let language_id = LanguageId::from_path(path)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Could not determine language ID for file: {}",
                    path.display()
                )
            })?
            .lsp_language_identifier()
            .to_owned();

        let did_open_params = DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: path_to_lsp_uri(path)?,
                language_id,
                version: initial_version as i32,
                text: content,
            },
        };

        self.service
            .send_notification::<notification::DidOpenTextDocument>(did_open_params)
    }

    pub async fn did_close(&self, path: &Path) -> Result<()> {
        {
            // Drop the guard before await
            let mut open_documents = self
                .open_documents
                .lock()
                .map_err(|_| anyhow::anyhow!("Failed to acquire lock on open documents"))?;
            if open_documents.remove(&path.to_path_buf()).is_none() {
                return Ok(());
            }
        }

        let did_close_params = DidCloseTextDocumentParams {
            text_document: TextDocumentIdentifier {
                uri: path_to_lsp_uri(path)?,
            },
        };

        self.service
            .send_notification::<notification::DidCloseTextDocument>(did_close_params)
    }

    pub async fn did_change(
        &self,
        path: &Path,
        version: i32,
        deltas: Vec<TextDocumentContentChangeEvent>,
    ) -> Result<()> {
        {
            // Check if document is open
            let mut open_documents = self
                .open_documents
                .lock()
                .map_err(|_| anyhow::anyhow!("Failed to acquire lock on open documents"))?;

            let Some(state) = open_documents.get_mut(path) else {
                return Ok(());
            };

            state.last_synced_version = Some(version as usize);
        }

        let did_change_params = DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier {
                uri: path_to_lsp_uri(path)?,
                version,
            },
            content_changes: deltas.into_iter().map(|delta| delta.into_lsp()).collect(),
        };

        self.service
            .send_notification::<notification::DidChangeTextDocument>(did_change_params)
    }

    pub async fn definition(
        &self,
        path: &Path,
        position: Position,
    ) -> anyhow::Result<Vec<LspDefinitionLocation>> {
        let uri = path_to_lsp_uri(path)?;

        let definition_params = GotoDefinitionParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
        };

        let result = self
            .service
            .send_request::<request::GotoDefinition>(definition_params)
            .await;

        if let Err(e) = &result {
            self.service.log_to_server_log(
                LspServerLogLevel::Error,
                format!("textDocument/definition failed: {e}"),
            );
        }

        // The LSP spec says textDocument/definition can return null when no definition is found
        // Handle this case explicitly since GotoDefinitionResponse doesn't deserialize null properly
        let Some(response) = result? else {
            return Err(anyhow::anyhow!("No definition found or LSP busy"));
        };

        match response {
            GotoDefinitionResponse::Scalar(location) => Ok(vec![location.into()]),
            GotoDefinitionResponse::Array(locations) => {
                Ok(locations.into_iter().map(Into::into).collect())
            }
            GotoDefinitionResponse::Link(locations) => {
                Ok(locations.into_iter().map(Into::into).collect())
            }
        }
    }

    pub async fn format(
        &self,
        path: &Path,
        options: FormattingOptions,
    ) -> anyhow::Result<Option<Vec<TextEdit>>> {
        let format_params = DocumentFormattingParams {
            text_document: TextDocumentIdentifier {
                uri: path_to_lsp_uri(path)?,
            },
            options,
            work_done_progress_params: Default::default(),
        };

        let result = self
            .service
            .send_request::<request::Formatting>(format_params)
            .await;

        if let Err(e) = &result {
            self.service.log_to_server_log(
                LspServerLogLevel::Error,
                format!("textDocument/formatting failed: {e}"),
            );
        }

        // The LSP spec says textDocument/formatting can return null when formatting is not supported
        result.map(|edits_option| {
            edits_option.map(|text_edits| text_edits.into_iter().map(Into::into).collect())
        })
    }

    pub async fn hover(
        &self,
        path: &Path,
        position: Position,
    ) -> anyhow::Result<Option<HoverResult>> {
        let uri = path_to_lsp_uri(path)?;

        let hover_params = HoverParams {
            text_document_position_params: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position,
            },
            work_done_progress_params: Default::default(),
        };

        let result = self
            .service
            .send_request::<request::HoverRequest>(hover_params)
            .await;

        if let Err(e) = &result {
            self.service.log_to_server_log(
                LspServerLogLevel::Error,
                format!("textDocument/hover failed: {e}"),
            );
        }

        Ok(result?.map(Into::into))
    }

    pub async fn references(
        &self,
        path: &Path,
        position: Position,
    ) -> anyhow::Result<Vec<ReferenceLocation>> {
        let uri = path_to_lsp_uri(path)?;

        let reference_params = ReferenceParams {
            text_document_position: TextDocumentPositionParams {
                text_document: TextDocumentIdentifier { uri },
                position,
            },
            work_done_progress_params: Default::default(),
            partial_result_params: Default::default(),
            context: lsp_types::ReferenceContext {
                include_declaration: true,
            },
        };

        let result = self
            .service
            .send_request::<request::References>(reference_params)
            .await;

        if let Err(e) = &result {
            self.service.log_to_server_log(
                LspServerLogLevel::Error,
                format!("textDocument/references failed: {e}"),
            );
        }

        // The LSP spec says textDocument/references can return null when no references are found
        Ok(result?
            .unwrap_or_default()
            .into_iter()
            .filter_map(|loc| ReferenceLocation::try_from(loc).ok())
            .collect())
    }
}
