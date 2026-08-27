use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use proc_macro_crate::{crate_name, FoundCrate};
use quote::{format_ident, quote};
use syn::{
    parse::Parse, parse::ParseStream, punctuated::Punctuated, FnArg, ItemFn, Meta, Pat, Token,
};

struct InstrumentArgs {
    metas: Punctuated<Meta, Token![,]>,
}

impl Parse for InstrumentArgs {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        Ok(Self {
            metas: Punctuated::parse_terminated(input)?,
        })
    }
}

/// Instruments a Tauri command span with Auditaur's IPC trace context.
///
/// The command remains an explicit opt-in Tauri command: keep `#[tauri::command]`
/// and keep an `auditaur_trace_context: Option<IpcTraceContext>` parameter.
#[proc_macro_attribute]
pub fn instrument_ipc(attr: TokenStream, item: TokenStream) -> TokenStream {
    expand_instrument_ipc(attr.into(), item.into()).into()
}

/// Defines and instruments a Tauri command span with Auditaur's IPC trace context.
///
/// This wraps `#[tauri::command]`, injects Auditaur IPC carrier arguments,
/// and reads the frontend `traceparent` sent by `@auditaur/api`.
#[proc_macro_attribute]
pub fn auditaur_command(attr: TokenStream, item: TokenStream) -> TokenStream {
    expand_auditaur_command(attr.into(), item.into()).into()
}

fn expand_instrument_ipc(attr: TokenStream2, item: TokenStream2) -> TokenStream2 {
    let args = match syn::parse2::<InstrumentArgs>(attr) {
        Ok(args) => args,
        Err(error) => return error.to_compile_error(),
    };

    let auditaur_crate = dependency_crate_path("tauri-plugin-auditaur", "tauri_plugin_auditaur");
    let tracing_crate = dependency_crate_path("tracing", "tracing");
    let traceparent_field = quote! {
        traceparent = #auditaur_crate::ipc_traceparent(auditaur_trace_context.as_ref())
    };
    let instrument_args = match merge_instrument_args(
        args,
        quote!(auditaur_trace_context),
        quote!(#traceparent_field),
    ) {
        Ok(args) => args,
        Err(error) => return error.to_compile_error(),
    };

    quote! {
        #[#tracing_crate::instrument(#(#instrument_args),*)]
        #item
    }
}

fn expand_auditaur_command(attr: TokenStream2, item: TokenStream2) -> TokenStream2 {
    let args = match syn::parse2::<InstrumentArgs>(attr) {
        Ok(args) => args,
        Err(error) => return error.to_compile_error(),
    };
    let mut function = match syn::parse2::<ItemFn>(item) {
        Ok(function) => function,
        Err(error) => return error.to_compile_error(),
    };

    let auditaur_crate = dependency_crate_path("tauri-plugin-auditaur", "tauri_plugin_auditaur");
    let tauri_crate = dependency_crate_path("tauri", "tauri");
    let tracing_crate = dependency_crate_path("tracing", "tracing");
    if function_has_argument(&function, "auditaur_trace_context") {
        return syn::Error::new_spanned(
            &function.sig.ident,
            "`#[tauri_plugin_auditaur::auditaur_command]` reserves the `auditaur_trace_context` argument; remove it or use `#[tauri::command]` with `#[tauri_plugin_auditaur::instrument_ipc]` instead",
        )
        .to_compile_error();
    }
    if function.sig.asyncness.is_some() && !return_type_is_result(&function.sig.output) {
        let message = "`#[tauri_plugin_auditaur::auditaur_command]` injects a borrowed `tauri::ipc::Request<'_>` argument, and async Tauri commands that take a reference input must return `Result`; change the return type to `Result<T, E>`, or use `#[tauri::command]` with `#[tauri_plugin_auditaur::instrument_ipc(...)]` and an `auditaur_trace_context: Option<IpcTraceContext>` argument to keep the bare return";
        let error = match &function.sig.output {
            syn::ReturnType::Type(_, _) => syn::Error::new_spanned(&function.sig.output, message),
            syn::ReturnType::Default => syn::Error::new_spanned(&function.sig.ident, message),
        };
        return error.to_compile_error();
    }
    let request_ident = unique_argument_ident(&function, "auditaur_request");
    let request_arg: FnArg = syn::parse_quote! {
        #request_ident: #tauri_crate::ipc::Request<'_>
    };
    function.sig.inputs.push(request_arg);
    let context_arg: FnArg = syn::parse_quote! {
        auditaur_trace_context: Option<#auditaur_crate::IpcTraceContext>
    };
    function.sig.inputs.push(context_arg);

    let traceparent_field = quote! {
        traceparent = #auditaur_crate::ipc_traceparent_from_request_or_context(
            &#request_ident,
            auditaur_trace_context.as_ref()
        )
    };
    let injected_skip_args = quote!(#request_ident, auditaur_trace_context);
    let instrument_args = match merge_instrument_args(
        args,
        quote!(#injected_skip_args),
        quote!(#traceparent_field),
    ) {
        Ok(args) => args,
        Err(error) => return error.to_compile_error(),
    };

    quote! {
        #[#tauri_crate::command]
        #[#tracing_crate::instrument(#(#instrument_args),*)]
        #function
    }
}

fn merge_instrument_args(
    args: InstrumentArgs,
    injected_skip_arg: TokenStream2,
    traceparent_field: TokenStream2,
) -> syn::Result<Vec<TokenStream2>> {
    let mut instrument_args = Vec::new();
    let mut skip_args = None;
    let mut fields_args = None;
    let mut skip_all = false;

    for meta in args.metas {
        if meta.path().is_ident("skip") {
            let Meta::List(list) = meta else {
                return Err(syn::Error::new_spanned(meta, "expected skip(...)"));
            };
            let existing = list.tokens;
            skip_args = Some(if existing.is_empty() {
                quote!(#injected_skip_arg)
            } else if token_stream_mentions(&existing, &injected_skip_arg.to_string()) {
                quote!(#existing)
            } else {
                quote!(#existing, #injected_skip_arg)
            });
        } else if meta.path().is_ident("skip_all") {
            skip_all = true;
            instrument_args.push(quote!(#meta));
        } else if meta.path().is_ident("fields") {
            let Meta::List(list) = meta else {
                return Err(syn::Error::new_spanned(meta, "expected fields(...)"));
            };
            let existing = list.tokens;
            fields_args = Some(if existing.is_empty() {
                traceparent_field.clone()
            } else if token_stream_mentions(&existing, "traceparent") {
                quote!(#existing)
            } else {
                quote!(#existing, #traceparent_field)
            });
        } else {
            instrument_args.push(quote!(#meta));
        }
    }

    let skip_args = skip_args.unwrap_or_else(|| quote!(#injected_skip_arg));
    let fields_args = fields_args.unwrap_or_else(|| quote!(#traceparent_field));
    if !skip_all {
        instrument_args.push(quote!(skip(#skip_args)));
    }
    instrument_args.push(quote!(fields(#fields_args)));

    Ok(instrument_args)
}

fn unique_argument_ident(function: &ItemFn, base: &str) -> syn::Ident {
    let mut candidate = format_ident!("{base}");
    let mut suffix = 2;
    while function_has_argument(function, &candidate.to_string()) {
        candidate = format_ident!("{base}_{suffix}");
        suffix += 1;
    }
    candidate
}

fn function_has_argument(function: &ItemFn, name: &str) -> bool {
    function.sig.inputs.iter().any(|arg| {
        matches!(
            arg,
            FnArg::Typed(pat_type)
                if matches!(pat_type.pat.as_ref(), Pat::Ident(ident) if ident.ident == name)
        )
    })
}

/// Best-effort check for whether a return type is a `Result`.
///
/// Matches when the last path segment's identifier ends with `Result`, so
/// `Result<..>`, `anyhow::Result<..>`, `crate::x::Result<..>`, and `Result`
/// type aliases such as `TauriCommandResult<..>`, `CmdResult<..>`, or
/// `IoResult<..>` all pass. A bare/unit return (`ReturnType::Default`) is
/// treated as not a `Result`. Non-path shapes (e.g. `impl Trait`, tuples) are
/// treated as `Result` so the guard prefers a false-negative (allow) over a
/// false-positive (wrongly rejecting a valid command) — a friendly diagnostic
/// must never turn currently-valid code into a compile error.
///
/// Limitation: a `Result` type alias whose name does not end in `Result`
/// (e.g. `type Outcome<T> = Result<T, E>;`) is not recognized, so an infallible
/// async command using such an alias would fall back to Tauri's opaque error
/// instead of this friendly one. That is the acceptable (false-negative) side.
fn return_type_is_result(output: &syn::ReturnType) -> bool {
    match output {
        syn::ReturnType::Default => false,
        syn::ReturnType::Type(_, ty) => type_ends_with_result(ty),
    }
}

fn type_ends_with_result(ty: &syn::Type) -> bool {
    match ty {
        syn::Type::Path(type_path) => type_path
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string().ends_with("Result"))
            .unwrap_or(false),
        _ => true,
    }
}

fn token_stream_mentions(tokens: &TokenStream2, needle: &str) -> bool {
    tokens
        .to_string()
        .split_whitespace()
        .any(|part| part == needle)
}

fn dependency_crate_path(package_name: &str, fallback_name: &str) -> TokenStream2 {
    match crate_name(package_name) {
        Ok(FoundCrate::Itself) => quote!(crate),
        Ok(FoundCrate::Name(name)) => {
            let ident = format_ident!("{}", name);
            quote!(::#ident)
        }
        Err(_) => {
            let ident = format_ident!("{}", fallback_name);
            quote!(::#ident)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{expand_auditaur_command, expand_instrument_ipc};
    use quote::quote;

    #[test]
    fn adds_traceparent_field_and_context_skip() {
        let expanded = expand_instrument_ipc(
            quote!(),
            quote! {
                fn load_user(id: String, auditaur_trace_context: Option<IpcTraceContext>) {}
            },
        )
        .to_string();

        assert!(expanded.contains("skip (auditaur_trace_context)"));
        assert!(expanded.contains("fields (traceparent ="));
        assert!(expanded.contains("ipc_traceparent"));
    }

    #[test]
    fn preserves_tracing_options_and_merges_skip_fields() {
        let expanded = expand_instrument_ipc(
            quote!(err, skip(app), fields(command = "emit_backend_event")),
            quote! {
                fn emit_backend_event(
                    app: tauri::AppHandle,
                    auditaur_trace_context: Option<IpcTraceContext>,
                ) {}
            },
        )
        .to_string();

        assert!(expanded.contains("err"));
        assert!(expanded.contains("skip (app , auditaur_trace_context)"));
        assert!(expanded.contains("command = \"emit_backend_event\""));
        assert!(expanded.contains("traceparent ="));
    }

    #[test]
    fn respects_skip_all_without_adding_skip() {
        let expanded = expand_instrument_ipc(
            quote!(skip_all),
            quote! {
                fn load_user(id: String, auditaur_trace_context: Option<IpcTraceContext>) {}
            },
        )
        .to_string();

        assert!(expanded.contains("skip_all"));
        assert!(!expanded.contains("skip (auditaur_trace_context)"));
        assert!(expanded.contains("fields (traceparent ="));
    }

    #[test]
    fn avoids_duplicate_injected_arguments() {
        let expanded = expand_instrument_ipc(
            quote!(skip(auditaur_trace_context), fields(traceparent = "custom")),
            quote! {
                fn load_user(id: String, auditaur_trace_context: Option<IpcTraceContext>) {}
            },
        )
        .to_string();

        assert!(expanded.contains("skip (auditaur_trace_context)"));
        assert!(!expanded.contains("auditaur_trace_context , auditaur_trace_context"));
        assert!(expanded.contains("traceparent"));
        assert!(expanded.contains("custom"));
        assert!(!expanded.contains("ipc_traceparent"));
    }

    #[test]
    fn auditaur_command_wraps_tauri_command_and_injects_request_traceparent() {
        let expanded = expand_auditaur_command(
            quote!(err, skip(app), fields(command = "load_user")),
            quote! {
                async fn load_user(app: tauri::AppHandle, id: String) -> Result<String, String> {
                    Ok(id)
                }
            },
        )
        .to_string();

        assert!(expanded.contains("tauri :: command"));
        assert!(expanded.contains("tracing :: instrument"));
        assert!(expanded.contains("auditaur_request :"));
        assert!(expanded.contains("tauri :: ipc :: Request < '_ >"));
        assert!(expanded.contains("auditaur_trace_context : Option"));
        assert!(expanded.contains("skip (app , auditaur_request , auditaur_trace_context)"));
        assert!(expanded.contains("ipc_traceparent_from_request_or_context"));
        assert!(expanded.contains("& auditaur_request"));
        assert!(expanded.contains("auditaur_trace_context . as_ref"));
        assert!(expanded.contains("command = \"load_user\""));
        assert!(expanded.contains("err"));
    }

    #[test]
    fn auditaur_command_avoids_request_argument_name_collision() {
        let expanded = expand_auditaur_command(
            quote!(),
            quote! {
                fn load_user(auditaur_request: String) -> String {
                    auditaur_request
                }
            },
        )
        .to_string();

        assert!(expanded.contains("auditaur_request_2 :"));
        assert!(expanded.contains("skip (auditaur_request_2 , auditaur_trace_context)"));
        assert!(expanded.contains("ipc_traceparent_from_request_or_context"));
        assert!(expanded.contains("& auditaur_request_2"));
    }

    #[test]
    fn auditaur_command_rejects_reserved_trace_context_argument() {
        let expanded = expand_auditaur_command(
            quote!(),
            quote! {
                fn load_user(auditaur_trace_context: Option<IpcTraceContext>) {}
            },
        )
        .to_string();

        assert!(expanded.contains("reserves the `auditaur_trace_context` argument"));
    }

    #[test]
    fn auditaur_command_rejects_async_non_result_return() {
        let expanded = expand_auditaur_command(
            quote!(),
            quote! {
                async fn copilot_auth_status() -> CopilotAuthStatus {
                    CopilotAuthStatus::default()
                }
            },
        )
        .to_string();

        assert!(expanded.contains("compile_error"));
        assert!(expanded.contains("must return `Result`"));
        assert!(expanded.contains("instrument_ipc"));
        assert!(!expanded.contains("tauri :: ipc :: Request"));
    }

    #[test]
    fn auditaur_command_rejects_async_unit_return() {
        let expanded = expand_auditaur_command(
            quote!(),
            quote! {
                async fn emit_ping() {}
            },
        )
        .to_string();

        assert!(expanded.contains("compile_error"));
        assert!(expanded.contains("must return `Result`"));
    }

    #[test]
    fn auditaur_command_allows_async_result_return() {
        let expanded = expand_auditaur_command(
            quote!(),
            quote! {
                async fn load_user(id: String) -> anyhow::Result<String> {
                    Ok(id)
                }
            },
        )
        .to_string();

        assert!(!expanded.contains("compile_error"));
        assert!(expanded.contains("tauri :: command"));
        assert!(expanded.contains("tauri :: ipc :: Request < '_ >"));
    }

    #[test]
    fn auditaur_command_allows_async_result_type_alias_return() {
        for return_type in [
            "TauriCommandResult<String>",
            "CmdResult<String>",
            "IoResult<()>",
        ] {
            let ret: proc_macro2::TokenStream = return_type.parse().unwrap();
            let expanded = expand_auditaur_command(
                quote!(),
                quote! {
                    async fn load_user(id: String) -> #ret {
                        Ok(id)
                    }
                },
            )
            .to_string();

            assert!(
                !expanded.contains("compile_error"),
                "alias `{return_type}` was wrongly flagged: {expanded}"
            );
            assert!(expanded.contains("tauri :: command"));
        }
    }

    #[test]
    fn auditaur_command_allows_sync_non_result_return() {
        let expanded = expand_auditaur_command(
            quote!(),
            quote! {
                fn app_name() -> String {
                    "auditaur".to_string()
                }
            },
        )
        .to_string();

        assert!(!expanded.contains("compile_error"));
        assert!(expanded.contains("tauri :: command"));
        assert!(expanded.contains("tauri :: ipc :: Request < '_ >"));
    }
}
