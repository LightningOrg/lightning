//! # lightning-macros
//!
//! The macros that make Lightning feel like magic:
//!
//! - `#[command]` — turns an async function into a slash **and** prefix command.
//! - `#[subcommand]` — nests a command inside another one, optionally in a group.
//! - `#[autocomplete]` — serves typed suggestions for one slash command option.
//! - `#[component]` — routes buttons, menus and modals by `custom_id`.
//! - `#[event]` — subscribes a handler to a Gateway event.
//! - `#[main]` — boots the runtime with tracing and the banner.
//! - `#[derive(CommandArgs)]` — generates Discord options and the prefix parser.

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{
    Attribute, Data, DeriveInput, Expr, ExprArray, Fields, FnArg, Ident, ItemFn, Lit, LitStr, Pat,
    Type, parse_macro_input, spanned::Spanned,
};

/// Marks a function as a Lightning command.
///
/// ```ignore
/// # use lightning::prelude::*;
/// /// Replies with pong
/// #[command]
/// async fn ping(ctx: Context) -> Reply {
///     Reply::text("pong")
/// }
/// ```
#[proc_macro_attribute]
pub fn command(attr: TokenStream, item: TokenStream) -> TokenStream {
    let function = parse_macro_input!(item as ItemFn);
    let cfg = match CommandAttr::parse(attr.into(), &function) {
        Ok(cfg) => cfg,
        Err(error) => return error.to_compile_error().into(),
    };

    match expand_command(cfg, function) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

/// Marks a function as a subcommand of another command.
///
/// ```ignore
/// # use lightning::prelude::*;
/// /// Manage the bot configuration
/// #[command]
/// async fn config() {}
///
/// /// Show the current configuration
/// #[subcommand(command = "config")]
/// async fn show() -> Reply {
///     Reply::text("everything is fine")
/// }
///
/// /// Change a setting
/// #[subcommand(command = "config", group = "set", group_description = "Change a setting")]
/// async fn channel(ctx: Context) -> Reply {
///     Reply::text("setting updated")
/// }
/// ```
#[proc_macro_attribute]
pub fn subcommand(attr: TokenStream, item: TokenStream) -> TokenStream {
    let function = parse_macro_input!(item as ItemFn);

    match expand_subcommand(attr.into(), function) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

/// Marks a function as the autocomplete handler for one command option.
///
/// The function accepts the same extractors as a command (`Context`,
/// `State<T>`, `Focused`…) and returns anything that implements `IntoChoices`:
/// a `Vec<AutocompleteChoice>`, a `Vec<String>`, a `Vec<&str>`…
///
/// ```ignore
/// # use lightning::prelude::*;
/// #[autocomplete(command = "search", option = "query")]
/// async fn search_query(focused: Focused) -> Vec<AutocompleteChoice> {
///     ["apple", "banana"]
///         .into_iter()
///         .filter(|fruit| fruit.contains(&focused.value))
///         .map(|fruit| AutocompleteChoice::text(fruit, fruit))
///         .collect()
/// }
/// ```
#[proc_macro_attribute]
pub fn autocomplete(attr: TokenStream, item: TokenStream) -> TokenStream {
    let function = parse_macro_input!(item as ItemFn);

    match expand_autocomplete(attr.into(), function) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

/// Marks a function as the handler for an interactive component.
///
/// ```ignore
/// # use lightning::prelude::*;
/// #[component("confirm")]
/// async fn on_confirm(ctx: Context) -> Reply {
///     Reply::text("confirmed!")
/// }
/// ```
#[proc_macro_attribute]
pub fn component(attr: TokenStream, item: TokenStream) -> TokenStream {
    let id = parse_macro_input!(attr as LitStr);
    let function = parse_macro_input!(item as ItemFn);

    match expand_component(id, function) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

/// Subscribes a function to a Gateway event.
///
/// The first parameter receives the event payload by reference; the second
/// (optional) one receives an `EventContext` with HTTP and state access.
///
/// ```ignore
/// # use lightning::prelude::*;
/// #[event(MemberAdd)]
/// async fn welcome(event: &MemberAdd) {
///     println!("{} joined", event.user.id);
/// }
/// ```
#[proc_macro_attribute]
pub fn event(attr: TokenStream, item: TokenStream) -> TokenStream {
    let variant = parse_macro_input!(attr as Ident);
    let function = parse_macro_input!(item as ItemFn);

    match expand_event(variant, function) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

/// Entry point with a Tokio runtime, tracing and banner included.
///
/// ```ignore
/// # use lightning::prelude::*;
/// #[lightning::main]
/// async fn main() {
///     println!("hello");
/// }
/// ```
#[proc_macro_attribute]
pub fn main(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let function = parse_macro_input!(item as ItemFn);

    match expand_main(function) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

/// Derives `CommandArgs` from a struct's fields.
///
/// ```ignore
/// # use lightning::prelude::*;
/// #[derive(CommandArgs)]
/// struct Echo {
///     /// What to repeat
///     text: String,
///     /// How many times (optional)
///     times: Option<u8>,
/// }
/// ```
#[proc_macro_derive(CommandArgs, attributes(arg))]
pub fn derive_command_args(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    match expand_command_args(input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

// ---------------------------------------------------------------------------
// #[command]
// ---------------------------------------------------------------------------

#[derive(Default)]
struct CommandAttr {
    name: Option<String>,
    description: Option<String>,
    kind: Option<String>,
    aliases: Vec<String>,
    guild_only: bool,
    nsfw: bool,
    permissions: Option<u64>,
    middleware: Vec<Expr>,
    auto_register: bool,
}

impl CommandAttr {
    fn parse(tokens: TokenStream2, function: &ItemFn) -> syn::Result<Self> {
        let mut cfg = Self::default();
        let attr: Attribute = syn::parse_quote!(#[command(#tokens)]);

        if !tokens.is_empty() {
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("name") {
                    cfg.name = Some(meta.value()?.parse::<LitStr>()?.value());
                    return Ok(());
                }
                if meta.path.is_ident("description") {
                    cfg.description = Some(meta.value()?.parse::<LitStr>()?.value());
                    return Ok(());
                }
                if meta.path.is_ident("kind") {
                    cfg.kind = Some(meta.value()?.parse::<LitStr>()?.value());
                    return Ok(());
                }
                if meta.path.is_ident("aliases") {
                    cfg.aliases = string_array(meta.value()?.parse::<Expr>()?)?;
                    return Ok(());
                }
                if meta.path.is_ident("middleware") {
                    cfg.middleware = expr_array(meta.value()?.parse::<Expr>()?)?;
                    return Ok(());
                }
                if meta.path.is_ident("permissions") {
                    let value: Expr = meta.value()?.parse()?;
                    if let Expr::Lit(lit) = &value {
                        if let Lit::Int(int) = &lit.lit {
                            cfg.permissions = Some(int.base10_parse()?);
                            return Ok(());
                        }
                    }
                    return Err(syn::Error::new(
                        value.span(),
                        "`permissions` expects an integer (Discord bits)",
                    ));
                }
                if meta.path.is_ident("guild_only") {
                    cfg.guild_only = true;
                    return Ok(());
                }
                if meta.path.is_ident("nsfw") {
                    cfg.nsfw = true;
                    return Ok(());
                }
                if meta.path.is_ident("auto_register") {
                    let value: LitStr = meta.value()?.parse()?;
                    cfg.auto_register = matches!(value.value().as_str(), "true" | "yes");
                    return Ok(());
                }

                Err(meta.error(
                    "unknown attribute: use name, description, kind, aliases, middleware, permissions, guild_only, nsfw or auto_register",
                ))
            })?;
        }

        if cfg.description.is_none() {
            cfg.description = doc_comment(&function.attrs);
        }

        Ok(cfg)
    }
}

fn expand_command(cfg: CommandAttr, function: ItemFn) -> syn::Result<TokenStream2> {
    let fn_name = &function.sig.ident;
    let name = cfg.name.clone().unwrap_or_else(|| fn_name.to_string());
    let description = cfg.description.clone().unwrap_or_default();
    let name_lit = LitStr::new(&name, fn_name.span());
    let desc_lit = LitStr::new(&description, fn_name.span());

    let kind = match cfg.kind.as_deref() {
        None | Some("both") => quote!(::lightning::CommandKind::Both),
        Some("slash") => quote!(::lightning::CommandKind::Slash),
        Some("prefix") => quote!(::lightning::CommandKind::Prefix),
        Some(other) => {
            return Err(syn::Error::new(
                fn_name.span(),
                format!("unknown kind `{other}`: use slash, prefix or both"),
            ));
        }
    };

    let run_ident = format_ident!("__lightning_run_{}", fn_name);
    let (run_fn, args_ty) = generate_runner(&run_ident, &function)?;

    let mut builder = quote! {
        ::lightning::Command::new(#name_lit, #desc_lit)
            .kind(#kind)
            .options(<#args_ty as ::lightning::CommandArgs>::OPTIONS)
            .handler(#run_ident)
    };

    if !cfg.aliases.is_empty() {
        let aliases = cfg
            .aliases
            .iter()
            .map(|alias| LitStr::new(alias, fn_name.span()));
        builder.extend(quote! { .aliases(&[#(#aliases),*]) });
    }
    if cfg.guild_only {
        builder.extend(quote! { .guild_only() });
    }
    if cfg.nsfw {
        builder.extend(quote! { .nsfw() });
    }
    if let Some(bits) = cfg.permissions {
        builder.extend(quote! { .permissions(#bits) });
    }
    if !cfg.middleware.is_empty() {
        let layers_ident = format_ident!("__lightning_layers_{}", fn_name);
        builder.extend(quote! { .layers(#layers_ident) });

        let expressions = &cfg.middleware;
        let layers = quote! {
            #[allow(non_snake_case)]
            fn #layers_ident() -> ::std::vec::Vec<::std::sync::Arc<dyn ::lightning::Middleware>> {
                ::std::vec![
                    #(::std::sync::Arc::new(#expressions) as ::std::sync::Arc<dyn ::lightning::Middleware>),*
                ]
            }
        };

        let registration = if cfg.auto_register {
            quote!()
        } else {
            quote! {
                ::lightning::inventory::submit! { #builder }
            }
        };

        return Ok(quote! {
            #function
            #run_fn
            #layers
            #registration
        });
    }

    let registration = if cfg.auto_register {
        quote!()
    } else {
        quote! {
            ::lightning::inventory::submit! { #builder }
        }
    };

    Ok(quote! {
        #function
        #run_fn
        #registration
    })
}

// ---------------------------------------------------------------------------
// #[subcommand]
// ---------------------------------------------------------------------------

#[derive(Default)]
struct SubcommandAttr {
    command: Option<String>,
    group: Option<String>,
    group_description: Option<String>,
    name: Option<String>,
    description: Option<String>,
    aliases: Vec<String>,
    middleware: Vec<Expr>,
}

impl SubcommandAttr {
    fn parse(tokens: TokenStream2, function: &ItemFn) -> syn::Result<Self> {
        let mut cfg = Self::default();
        let attr: Attribute = syn::parse_quote!(#[subcommand(#tokens)]);

        if !tokens.is_empty() {
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("command") {
                    cfg.command = Some(meta.value()?.parse::<LitStr>()?.value());
                    return Ok(());
                }
                if meta.path.is_ident("group") {
                    cfg.group = Some(meta.value()?.parse::<LitStr>()?.value());
                    return Ok(());
                }
                if meta.path.is_ident("group_description") {
                    cfg.group_description = Some(meta.value()?.parse::<LitStr>()?.value());
                    return Ok(());
                }
                if meta.path.is_ident("name") {
                    cfg.name = Some(meta.value()?.parse::<LitStr>()?.value());
                    return Ok(());
                }
                if meta.path.is_ident("description") {
                    cfg.description = Some(meta.value()?.parse::<LitStr>()?.value());
                    return Ok(());
                }
                if meta.path.is_ident("aliases") {
                    cfg.aliases = string_array(meta.value()?.parse::<Expr>()?)?;
                    return Ok(());
                }
                if meta.path.is_ident("middleware") {
                    cfg.middleware = expr_array(meta.value()?.parse::<Expr>()?)?;
                    return Ok(());
                }

                Err(meta.error(
                    "unknown attribute: use command, group, group_description, name, description, aliases or middleware",
                ))
            })?;
        }

        if cfg.command.is_none() {
            return Err(syn::Error::new_spanned(
                &function.sig,
                "`#[subcommand]` requires `command = \"...\"`",
            ));
        }
        if cfg.group.is_none() && cfg.group_description.is_some() {
            return Err(syn::Error::new_spanned(
                &function.sig,
                "`group_description` requires `group = \"...\"`",
            ));
        }
        if cfg.description.is_none() {
            cfg.description = doc_comment(&function.attrs);
        }

        Ok(cfg)
    }
}

fn expand_subcommand(tokens: TokenStream2, function: ItemFn) -> syn::Result<TokenStream2> {
    let cfg = SubcommandAttr::parse(tokens, &function)?;
    let fn_name = &function.sig.ident;
    let command = cfg.command.clone().expect("validated by SubcommandAttr");
    let name = cfg.name.clone().unwrap_or_else(|| fn_name.to_string());
    let description = cfg.description.clone().unwrap_or_default();

    let command_lit = LitStr::new(&command, fn_name.span());
    let name_lit = LitStr::new(&name, fn_name.span());
    let description_lit = LitStr::new(&description, fn_name.span());
    let run_ident = format_ident!("__lightning_subcommand_{}", fn_name);
    let (run_fn, args_ty) = generate_runner(&run_ident, &function)?;

    let mut builder = quote! {
        ::lightning::Subcommand::new(#command_lit, #name_lit, #description_lit)
            .options(<#args_ty as ::lightning::CommandArgs>::OPTIONS)
            .handler(#run_ident)
    };

    if let Some(group) = &cfg.group {
        let group_lit = LitStr::new(group, fn_name.span());
        let group_description = cfg.group_description.clone().unwrap_or_default();
        let group_description_lit = LitStr::new(&group_description, fn_name.span());
        builder.extend(quote! { .group(#group_lit, #group_description_lit) });
    }

    if !cfg.aliases.is_empty() {
        let aliases = cfg
            .aliases
            .iter()
            .map(|alias| LitStr::new(alias, fn_name.span()));
        builder.extend(quote! { .aliases(&[#(#aliases),*]) });
    }

    if !cfg.middleware.is_empty() {
        let layers_ident = format_ident!("__lightning_subcommand_layers_{}", fn_name);
        builder.extend(quote! { .layers(#layers_ident) });

        let expressions = &cfg.middleware;
        let layers = quote! {
            #[allow(non_snake_case)]
            fn #layers_ident() -> ::std::vec::Vec<::std::sync::Arc<dyn ::lightning::Middleware>> {
                ::std::vec![
                    #(::std::sync::Arc::new(#expressions) as ::std::sync::Arc<dyn ::lightning::Middleware>),*,
                ]
            }
        };

        return Ok(quote! {
            #function
            #run_fn
            #layers

            ::lightning::inventory::submit! { #builder }
        });
    }

    Ok(quote! {
        #function
        #run_fn

        ::lightning::inventory::submit! { #builder }
    })
}

/// Pieces shared by the `#[command]`, `#[subcommand]`, `#[component]` and
/// `#[autocomplete]` runners: how to obtain each parameter from the `Context`.
struct RunnerParts {
    extractions: Vec<TokenStream2>,
    call_args: Vec<TokenStream2>,
    args_ty: Type,
}

fn runner_parts(function: &ItemFn) -> syn::Result<RunnerParts> {
    let mut extractions = Vec::new();
    let mut call_args = Vec::new();
    let mut args_ty: Option<Type> = None;

    for input in &function.sig.inputs {
        let FnArg::Typed(arg) = input else {
            return Err(syn::Error::new_spanned(
                input,
                "commands cannot take `self`",
            ));
        };

        let Pat::Ident(pat) = &*arg.pat else {
            return Err(syn::Error::new_spanned(
                &arg.pat,
                "use a plain parameter name (no patterns)",
            ));
        };

        let ident = &pat.ident;
        let ty = &*arg.ty;
        extractions.push(quote! {
            let #ident: #ty = <#ty as ::lightning::FromContext>::from_context(&__ctx).await?;
        });
        call_args.push(quote!(#ident));

        if let Some(inner) = args_generic(ty) {
            args_ty = Some(inner);
        }
    }

    Ok(RunnerParts {
        extractions,
        call_args,
        args_ty: args_ty.unwrap_or_else(|| syn::parse_quote!(())),
    })
}

/// Generates the function that extracts arguments and calls the handler.
fn generate_runner(
    run_ident: &Ident,
    function: &ItemFn,
) -> syn::Result<(TokenStream2, TokenStream2)> {
    let RunnerParts {
        extractions,
        call_args,
        args_ty,
    } = runner_parts(function)?;
    let handler = &function.sig.ident;

    let run_fn = quote! {
        #[allow(non_snake_case)]
        fn #run_ident<'__lightning>(
            __ctx: &'__lightning ::lightning::Context,
        ) -> ::lightning::BoxFuture<'__lightning, ::lightning::Result<::lightning::Reply>> {
            ::std::boxed::Box::pin(async move {
                let __ctx: ::lightning::Context =
                    <::lightning::Context as ::lightning::FromContext>::from_context(__ctx).await?;
                #(#extractions)*
                ::lightning::IntoReply::into_reply(#handler(#(#call_args),*).await)
            })
        }
    };

    Ok((run_fn, quote!(#args_ty)))
}

// ---------------------------------------------------------------------------
// #[autocomplete]
// ---------------------------------------------------------------------------

#[derive(Default)]
struct AutocompleteAttr {
    command: Option<String>,
    group: Option<String>,
    subcommand: Option<String>,
    option: Option<String>,
}

impl AutocompleteAttr {
    fn parse(tokens: TokenStream2) -> syn::Result<Self> {
        let mut cfg = Self::default();
        let attr: Attribute = syn::parse_quote!(#[autocomplete(#tokens)]);

        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("command") {
                cfg.command = Some(meta.value()?.parse::<LitStr>()?.value());
                return Ok(());
            }
            if meta.path.is_ident("group") {
                cfg.group = Some(meta.value()?.parse::<LitStr>()?.value());
                return Ok(());
            }
            if meta.path.is_ident("subcommand") {
                cfg.subcommand = Some(meta.value()?.parse::<LitStr>()?.value());
                return Ok(());
            }
            if meta.path.is_ident("option") {
                cfg.option = Some(meta.value()?.parse::<LitStr>()?.value());
                return Ok(());
            }

            Err(meta.error("unknown attribute: use command, group, subcommand and option"))
        })?;

        if cfg.group.is_some() && cfg.subcommand.is_none() {
            return Err(syn::Error::new_spanned(
                &attr,
                "`group` requires `subcommand = \"...\"`",
            ));
        }

        Ok(cfg)
    }
}

fn expand_autocomplete(tokens: TokenStream2, function: ItemFn) -> syn::Result<TokenStream2> {
    let cfg = AutocompleteAttr::parse(tokens)?;
    let fn_name = &function.sig.ident;

    let command = cfg.command.ok_or_else(|| {
        syn::Error::new_spanned(
            &function.sig,
            "`#[autocomplete]` requires `command = \"...\"`",
        )
    })?;
    let option = cfg.option.ok_or_else(|| {
        syn::Error::new_spanned(
            &function.sig,
            "`#[autocomplete]` requires `option = \"...\"`",
        )
    })?;

    let command_lit = LitStr::new(&command, fn_name.span());
    let option_lit = LitStr::new(&option, fn_name.span());
    let group = match &cfg.group {
        Some(group) => {
            let group_lit = LitStr::new(group, fn_name.span());
            quote!(::std::option::Option::Some(#group_lit))
        }
        None => quote!(::std::option::Option::None),
    };
    let subcommand = match &cfg.subcommand {
        Some(subcommand) => {
            let subcommand_lit = LitStr::new(subcommand, fn_name.span());
            quote!(::std::option::Option::Some(#subcommand_lit))
        }
        None => quote!(::std::option::Option::None),
    };
    let run_ident = format_ident!("__lightning_autocomplete_{}", fn_name);
    let RunnerParts {
        extractions,
        call_args,
        ..
    } = runner_parts(&function)?;

    Ok(quote! {
        #function

        #[allow(non_snake_case)]
        fn #run_ident<'__lightning>(
            __ctx: &'__lightning ::lightning::Context,
        ) -> ::lightning::BoxFuture<
            '__lightning,
            ::lightning::Result<::std::vec::Vec<::lightning::AutocompleteChoice>>,
        > {
            ::std::boxed::Box::pin(async move {
                let __ctx: ::lightning::Context =
                    <::lightning::Context as ::lightning::FromContext>::from_context(__ctx).await?;
                #(#extractions)*
                ::lightning::IntoChoices::into_choices(#fn_name(#(#call_args),*).await)
            })
        }

        ::lightning::inventory::submit! {
            ::lightning::AutocompleteHandler {
                command: #command_lit,
                group: #group,
                subcommand: #subcommand,
                option: #option_lit,
                run: #run_ident,
            }
        }
    })
}

// ---------------------------------------------------------------------------
// #[component]
// ---------------------------------------------------------------------------

fn expand_component(id: LitStr, function: ItemFn) -> syn::Result<TokenStream2> {
    let fn_name = &function.sig.ident;
    let run_ident = format_ident!("__lightning_run_{}", fn_name);
    let (run_fn, _) = generate_runner(&run_ident, &function)?;

    Ok(quote! {
        #function
        #run_fn

        ::lightning::inventory::submit! {
            ::lightning::ComponentHandler {
                id: #id,
                run: #run_ident,
            }
        }
    })
}

// ---------------------------------------------------------------------------
// #[event]
// ---------------------------------------------------------------------------

fn expand_event(variant: Ident, function: ItemFn) -> syn::Result<TokenStream2> {
    let fn_name = &function.sig.ident;
    let handle_ident = format_ident!("__lightning_event_{}", fn_name);
    let event_name = variant.to_string();

    let mut params = function.sig.inputs.iter();
    if params.next().is_none() {
        return Err(syn::Error::new_spanned(
            &function.sig,
            "event handlers must take the payload as their first parameter",
        ));
    }
    let with_ctx = params.next().is_some();

    let payload = quote!(__payload);
    let call = if with_ctx {
        quote!(#fn_name(#payload, __ctx).await)
    } else {
        quote!(#fn_name(#payload).await)
    };

    Ok(quote! {
        #function

        #[allow(non_snake_case)]
        fn #handle_ident<'__lightning>(
            __event: &'__lightning ::lightning::Event,
            __ctx: ::lightning::EventContext,
        ) -> ::lightning::BoxFuture<'__lightning, ()> {
            ::std::boxed::Box::pin(async move {
                if let ::lightning::Event::#variant(__payload) = __event {
                    let _ = #call;
                }
            })
        }

        ::lightning::inventory::submit! {
            ::lightning::EventHandler {
                name: #event_name,
                handle: #handle_ident,
            }
        }
    })
}

// ---------------------------------------------------------------------------
// #[main]
// ---------------------------------------------------------------------------

fn expand_main(function: ItemFn) -> syn::Result<TokenStream2> {
    if function.sig.asyncness.is_none() {
        return Err(syn::Error::new_spanned(
            &function.sig,
            "#[lightning::main] requires an `async fn main()`",
        ));
    }
    if !function.sig.inputs.is_empty() {
        return Err(syn::Error::new_spanned(
            &function.sig,
            "`main` cannot take parameters",
        ));
    }

    let attrs = &function.attrs;
    let body = &function.block;

    Ok(quote! {
        #(#attrs)*
        fn main() {
            ::lightning::__private::runtime_main(async move #body)
        }
    })
}

// ---------------------------------------------------------------------------
// derive(CommandArgs)
// ---------------------------------------------------------------------------

struct FieldInfo {
    ident: Ident,
    ty: Type,
    name: String,
    description: String,
    rest: bool,
}

fn expand_command_args(input: DeriveInput) -> syn::Result<TokenStream2> {
    let name = &input.ident;
    let Data::Struct(data) = &input.data else {
        return Err(syn::Error::new_spanned(
            &input.ident,
            "CommandArgs can only be derived for structs",
        ));
    };
    let Fields::Named(fields) = &data.fields else {
        return Err(syn::Error::new_spanned(
            &data.fields,
            "CommandArgs requires a struct with named fields",
        ));
    };
    if !input.generics.params.is_empty() {
        return Err(syn::Error::new_spanned(
            &input.generics,
            "CommandArgs does not support generics",
        ));
    }

    let mut infos = Vec::new();
    for field in &fields.named {
        let ident = field.ident.clone().expect("named fields");
        let mut name = ident.to_string().trim_start_matches("r#").to_owned();
        let mut description: Option<String> = doc_comment(&field.attrs);
        let mut rest = false;

        for attr in &field.attrs {
            if !attr.path().is_ident("arg") {
                continue;
            }
            attr.parse_nested_meta(|meta| {
                if meta.path.is_ident("name") {
                    name = meta.value()?.parse::<LitStr>()?.value();
                    return Ok(());
                }
                if meta.path.is_ident("description") {
                    description = Some(meta.value()?.parse::<LitStr>()?.value());
                    return Ok(());
                }
                if meta.path.is_ident("rest") {
                    rest = true;
                    return Ok(());
                }

                Err(meta.error("unknown attribute: use name, description or rest"))
            })?;
        }

        let description = description.unwrap_or_else(|| humanize(&name));

        infos.push(FieldInfo {
            ident,
            ty: field.ty.clone(),
            name,
            description,
            rest,
        });
    }

    let specs = infos.iter().map(|field| {
        let name = LitStr::new(&field.name, field.ident.span());
        let description = LitStr::new(&field.description, field.ident.span());
        let optional = option_inner(&field.ty).is_some();
        let required = !optional && !field.rest;
        let kind = option_kind(&field.ty);

        quote! {
            ::lightning::OptionSpec {
                name: #name,
                description: #description,
                kind: #kind,
                required: #required,
            }
        }
    });

    let option_reads = infos.iter().map(|field| {
        let ident = &field.ident;
        let ty = &field.ty;
        let name = LitStr::new(&field.name, field.ident.span());

        if field.rest && is_string(ty) {
            return quote! {
                let #ident: #ty = match __options.remove(#name) {
                    ::std::option::Option::Some(value) => {
                        <#ty as ::lightning::FromOptionValue>::from_option(value, #name)?
                    }
                    ::std::option::Option::None => ::std::string::String::new(),
                };
            };
        }

        quote! {
            let #ident: #ty = <#ty as ::lightning::FromOptionValue>::from_option(
                __options.remove(#name).unwrap_or(::lightning::serde_json::Value::Null),
                #name,
            )?;
        }
    });

    let prefix_reads = infos.iter().map(|field| {
        let ident = &field.ident;
        let ty = &field.ty;
        let name = LitStr::new(&field.name, field.ident.span());

        if field.rest {
            if is_string(ty) {
                return quote! {
                    let #ident: #ty = __tokens[__index..].join(" ");
                    __index = __tokens.len();
                };
            }
            if let Some(inner) = vec_inner(ty) {
                return quote! {
                    let #ident: #ty = __tokens[__index..]
                        .iter()
                        .map(|token| <#inner as ::lightning::FromToken>::from_token(token, #name))
                        .collect::<::lightning::Result<::std::vec::Vec<#inner>>>()?;
                    __index = __tokens.len();
                };
            }
        }

        if let Some(inner) = option_inner(ty) {
            return quote! {
                let #ident: #ty = if let ::std::option::Option::Some(token) = __tokens.get(__index) {
                    __index += 1;
                    ::std::option::Option::Some(
                        <#inner as ::lightning::FromToken>::from_token(token, #name)?,
                    )
                } else {
                    ::std::option::Option::None
                };
            };
        }

        quote! {
            let #ident: #ty = {
                let token = __tokens
                    .get(__index)
                    .ok_or_else(|| ::lightning::args::missing(#name))?;
                __index += 1;
                <#ty as ::lightning::FromToken>::from_token(token, #name)?
            };
        }
    });

    let field_idents: Vec<&Ident> = infos.iter().map(|field| &field.ident).collect();

    Ok(quote! {
        impl ::lightning::CommandArgs for #name {
            const OPTIONS: &'static [::lightning::OptionSpec] = &[#(#specs),*];

            fn from_options(
                mut __options: ::lightning::serde_json::Map<
                    ::std::string::String,
                    ::lightning::serde_json::Value,
                >,
            ) -> ::lightning::Result<Self> {
                #(#option_reads)*
                ::std::result::Result::Ok(Self { #(#field_idents),* })
            }

            fn from_prefix(__tokens: &[::std::string::String]) -> ::lightning::Result<Self> {
                let mut __index = 0usize;
                #(#prefix_reads)*
                ::std::result::Result::Ok(Self { #(#field_idents),* })
            }
        }
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn doc_comment(attrs: &[Attribute]) -> Option<String> {
    let mut lines = Vec::new();

    for attr in attrs {
        if !attr.path().is_ident("doc") {
            continue;
        }
        let syn::Meta::NameValue(meta) = &attr.meta else {
            continue;
        };
        let Expr::Lit(expr) = &meta.value else {
            continue;
        };
        let Lit::Str(text) = &expr.lit else {
            continue;
        };
        lines.push(text.value().trim().to_owned());
    }

    if lines.is_empty() {
        None
    } else {
        Some(lines.join("\n"))
    }
}

fn humanize(name: &str) -> String {
    name.replace(['_', '-'], " ")
}

fn string_array(expr: Expr) -> syn::Result<Vec<String>> {
    let Expr::Array(ExprArray { elems, .. }) = expr else {
        return Err(syn::Error::new_spanned(
            expr,
            "expected a list, for example `[\"p\", \"pi\"]`",
        ));
    };

    elems
        .into_iter()
        .map(|elem| match elem {
            Expr::Lit(lit) => match lit.lit {
                Lit::Str(text) => Ok(text.value()),
                _ => Err(syn::Error::new_spanned(lit, "expected a string")),
            },
            other => Err(syn::Error::new_spanned(other, "expected a string")),
        })
        .collect()
}

fn expr_array(expr: Expr) -> syn::Result<Vec<Expr>> {
    let Expr::Array(ExprArray { elems, .. }) = expr else {
        return Err(syn::Error::new_spanned(
            expr,
            "expected a middleware list, for example `[Cooldown::per_user(...)]`",
        ));
    };

    Ok(elems.into_iter().collect())
}

/// Returns `T` if the type is `Args<T>`.
fn args_generic(ty: &Type) -> Option<Type> {
    let Type::Path(path) = ty else {
        return None;
    };
    let last = path.path.segments.last()?;
    if last.ident != "Args" {
        return None;
    }
    let syn::PathArguments::AngleBracketed(args) = &last.arguments else {
        return None;
    };

    args.args.iter().find_map(|arg| match arg {
        syn::GenericArgument::Type(inner) => Some(inner.clone()),
        _ => None,
    })
}

fn option_inner(ty: &Type) -> Option<&Type> {
    generic_inner(ty, "Option")
}

fn vec_inner(ty: &Type) -> Option<&Type> {
    generic_inner(ty, "Vec")
}

fn generic_inner<'a>(ty: &'a Type, ident: &str) -> Option<&'a Type> {
    let Type::Path(path) = ty else {
        return None;
    };
    let last = path.path.segments.last()?;
    if last.ident != ident {
        return None;
    }
    let syn::PathArguments::AngleBracketed(args) = &last.arguments else {
        return None;
    };

    args.args.iter().find_map(|arg| match arg {
        syn::GenericArgument::Type(inner) => Some(inner),
        _ => None,
    })
}

fn is_string(ty: &Type) -> bool {
    matches!(ty, Type::Path(path) if path.path.segments.last().is_some_and(|s| s.ident == "String"))
}

fn option_kind(ty: &Type) -> TokenStream2 {
    let inner = option_inner(ty).or_else(|| vec_inner(ty)).unwrap_or(ty);

    let Type::Path(path) = inner else {
        return quote!(::lightning::OptionKind::String);
    };
    let Some(segment) = path.path.segments.last() else {
        return quote!(::lightning::OptionKind::String);
    };

    match segment.ident.to_string().as_str() {
        "i8" | "i16" | "i32" | "i64" | "i128" | "isize" | "u8" | "u16" | "u32" | "u64" | "u128"
        | "usize" => quote!(::lightning::OptionKind::Integer),
        "f32" | "f64" => quote!(::lightning::OptionKind::Number),
        "bool" => quote!(::lightning::OptionKind::Boolean),
        _ => quote!(::lightning::OptionKind::String),
    }
}
