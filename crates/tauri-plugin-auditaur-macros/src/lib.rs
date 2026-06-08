use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use proc_macro_crate::{crate_name, FoundCrate};
use quote::{format_ident, quote};
use syn::{parse::Parse, parse::ParseStream, punctuated::Punctuated, Meta, Token};

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
    let mut instrument_args = Vec::new();
    let mut skip_args = None;
    let mut fields_args = None;
    let mut skip_all = false;

    for meta in args.metas {
        if meta.path().is_ident("skip") {
            let Meta::List(list) = meta else {
                return syn::Error::new_spanned(meta, "expected skip(...)").to_compile_error();
            };
            let existing = list.tokens;
            skip_args = Some(if existing.is_empty() {
                quote!(auditaur_trace_context)
            } else if token_stream_mentions(&existing, "auditaur_trace_context") {
                quote!(#existing)
            } else {
                quote!(#existing, auditaur_trace_context)
            });
        } else if meta.path().is_ident("skip_all") {
            skip_all = true;
            instrument_args.push(quote!(#meta));
        } else if meta.path().is_ident("fields") {
            let Meta::List(list) = meta else {
                return syn::Error::new_spanned(meta, "expected fields(...)").to_compile_error();
            };
            let existing = list.tokens;
            fields_args = Some(if existing.is_empty() {
                quote!(#traceparent_field)
            } else if token_stream_mentions(&existing, "traceparent") {
                quote!(#existing)
            } else {
                quote!(#existing, #traceparent_field)
            });
        } else {
            instrument_args.push(quote!(#meta));
        }
    }

    let skip_args = skip_args.unwrap_or_else(|| quote!(auditaur_trace_context));
    let fields_args = fields_args.unwrap_or_else(|| quote!(#traceparent_field));
    if !skip_all {
        instrument_args.push(quote!(skip(#skip_args)));
    }
    instrument_args.push(quote!(fields(#fields_args)));

    quote! {
        #[#tracing_crate::instrument(#(#instrument_args),*)]
        #item
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
    use super::expand_instrument_ipc;
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
}
