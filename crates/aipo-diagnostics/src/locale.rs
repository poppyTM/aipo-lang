//! Internationalization (i18n) support for Aipo diagnostic catalog.
//!
//! Provides English (canonical standard default) and Brazilian Portuguese (`pt-BR`)
//! localized descriptions, titles, and labels for all stable diagnostic codes.
//!
//! Selection follows the cascading priority:
//! 1. Explicit API / CLI override
//! 2. `AIPO_LANG` environment variable
//! 3. `LC_ALL`, `LC_MESSAGES`, `LANG` OS environment variables
//! 4. Default: `Locale::En`.

use crate::code::DiagnosticCode;
use crate::severity::Severity;

/// Supported diagnostic locales.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Locale {
    /// English (canonical standard default).
    #[default]
    En,
    /// Brazilian Portuguese (`pt-BR`).
    PtBr,
}

impl Locale {
    /// Parses a locale identifier string (e.g. "en", "pt", "pt-BR", "pt_BR.UTF-8").
    ///
    /// The infallible signature predates `FromStr` conventions and is public API;
    /// keep the name and silence the trait-confusion lint locally.
    #[must_use]
    #[allow(clippy::should_implement_trait)]
    pub fn from_str(raw: &str) -> Self {
        let trimmed = raw.trim().to_lowercase();
        if trimmed.starts_with("pt") {
            Self::PtBr
        } else {
            Self::En
        }
    }

    /// Automatically detects the active locale by inspecting environment variables.
    ///
    /// Respects `AIPO_LANG` when set, defaulting to canonical `Locale::En`.
    #[must_use]
    pub fn detect() -> Self {
        if let Ok(val) = std::env::var("AIPO_LANG")
            && !val.trim().is_empty()
        {
            return Self::from_str(&val);
        }
        Self::En
    }

    /// Localized name of the severity level.
    #[must_use]
    pub const fn severity_label(self, severity: Severity) -> &'static str {
        match (self, severity) {
            (Self::En, Severity::Error) => "error",
            (Self::En, Severity::Fault) => "fault",
            (Self::En, Severity::Warning) => "warning",
            (Self::En, Severity::Note) => "note",
            (Self::PtBr, Severity::Error) => "erro",
            (Self::PtBr, Severity::Fault) => "falha",
            (Self::PtBr, Severity::Warning) => "aviso",
            (Self::PtBr, Severity::Note) => "nota",
        }
    }

    /// Localized label for secondary notes ("note:" / "nota:").
    #[must_use]
    pub const fn note_label(self) -> &'static str {
        match self {
            Self::En => "note",
            Self::PtBr => "nota",
        }
    }

    /// Localized label for help / suggestions ("help:" / "dica:").
    #[must_use]
    pub const fn help_label(self) -> &'static str {
        match self {
            Self::En => "help",
            Self::PtBr => "dica",
        }
    }
}

impl DiagnosticCode {
    /// Returns the localized short title / summary for this diagnostic code.
    #[must_use]
    pub fn title(self, locale: Locale) -> &'static str {
        match (locale, self) {
            (Locale::En, Self::AIPO_COMPILE_REG_UNSUPPORTED) => {
                "program is unsupported by the experimental register backend"
            }
            (Locale::PtBr, Self::AIPO_COMPILE_REG_UNSUPPORTED) => {
                "programa não suportado pelo backend experimental de registradores"
            }
            // Source
            (Locale::En, Self::AIPO_SRC_INVALID_UTF8) => "invalid UTF-8 sequence in source file",
            (Locale::PtBr, Self::AIPO_SRC_INVALID_UTF8) => {
                "sequência UTF-8 inválida no arquivo fonte"
            }

            // Lexical
            (Locale::En, Self::AIPO_LEX_UNTERMINATED_STRING) => "unterminated string literal",
            (Locale::PtBr, Self::AIPO_LEX_UNTERMINATED_STRING) => "literal de texto não finalizado",
            (Locale::En, Self::AIPO_LEX_UNKNOWN_ESCAPE) => "unknown escape sequence",
            (Locale::PtBr, Self::AIPO_LEX_UNKNOWN_ESCAPE) => "sequência de escape desconhecida",
            (Locale::En, Self::AIPO_LEX_INVALID_UNICODE_ESCAPE) => {
                "invalid Unicode escape sequence"
            }
            (Locale::PtBr, Self::AIPO_LEX_INVALID_UNICODE_ESCAPE) => {
                "sequência de escape Unicode inválida"
            }
            (Locale::En, Self::AIPO_LEX_INVALID_NUMBER) => "malformed numeric literal",
            (Locale::PtBr, Self::AIPO_LEX_INVALID_NUMBER) => "literal numérico malformado",
            (Locale::En, Self::AIPO_LEX_UNEXPECTED_CHARACTER) => "unexpected character",
            (Locale::PtBr, Self::AIPO_LEX_UNEXPECTED_CHARACTER) => "caractere inesperado",

            // Parse
            (Locale::En, Self::AIPO_PARSE_UNCLOSED_BLOCK) => {
                "unclosed block, parenthesis or delimiter"
            }
            (Locale::PtBr, Self::AIPO_PARSE_UNCLOSED_BLOCK) => {
                "bloco, parêntese ou delimitador não fechado"
            }
            (Locale::En, Self::AIPO_PARSE_UNEXPECTED_TOKEN) => {
                "unexpected token in syntactic construct"
            }
            (Locale::PtBr, Self::AIPO_PARSE_UNEXPECTED_TOKEN) => {
                "token inesperado na construção sintática"
            }
            (Locale::En, Self::AIPO_PARSE_MISSING_END) => "missing block closure",
            (Locale::PtBr, Self::AIPO_PARSE_MISSING_END) => "fechamento de bloco ausente",
            (Locale::En, Self::AIPO_PARSE_INVALID_TARGET) => "invalid assignment target path",
            (Locale::PtBr, Self::AIPO_PARSE_INVALID_TARGET) => "alvo de atribuição inválido",
            (Locale::En, Self::AIPO_PARSE_NESTING_TOO_DEEP) => {
                "nesting depth exceeds compiler limits"
            }
            (Locale::PtBr, Self::AIPO_PARSE_NESTING_TOO_DEEP) => {
                "profundidade de aninhamento excede o limite do compilador"
            }
            (Locale::En, Self::AIPO_PARSE_LOTE_INVALIDO) => {
                "invalid batch association syntax; expected bracketed list"
            }
            (Locale::PtBr, Self::AIPO_PARSE_LOTE_INVALIDO) => {
                "sintaxe de lote inválida; esperado lista entre colchetes"
            }

            // Semantic
            (Locale::En, Self::AIPO_SEM_UNKNOWN_NAME) => "unknown identifier or unresolved name",
            (Locale::PtBr, Self::AIPO_SEM_UNKNOWN_NAME) => "identificador ou nome desconhecido",
            (Locale::En, Self::AIPO_SEM_REDECLARED_IN_SCOPE) => {
                "identifier already declared in this scope"
            }
            (Locale::PtBr, Self::AIPO_SEM_REDECLARED_IN_SCOPE) => {
                "identificador já declarado neste escopo"
            }
            (Locale::En, Self::AIPO_SEM_READONLY_MUTATION) => {
                "attempted mutation of an immutable path"
            }
            (Locale::PtBr, Self::AIPO_SEM_READONLY_MUTATION) => {
                "tentativa de mutação em caminho imutável"
            }
            (Locale::En, Self::AIPO_SEM_FIXED_REASSIGN) => {
                "cannot reassign fixed field after initialization"
            }
            (Locale::PtBr, Self::AIPO_SEM_FIXED_REASSIGN) => {
                "campo fixo não pode ser reatribuído após inicialização"
            }
            (Locale::En, Self::AIPO_SEM_ARITY_MISMATCH) => {
                "argument count does not match signature"
            }
            (Locale::PtBr, Self::AIPO_SEM_ARITY_MISMATCH) => {
                "quantidade de argumentos não confere com a assinatura"
            }
            (Locale::En, Self::AIPO_SEM_NAMED_ARG_UNKNOWN) => "unknown named argument",
            (Locale::PtBr, Self::AIPO_SEM_NAMED_ARG_UNKNOWN) => "argumento nomeado desconhecido",
            (Locale::En, Self::AIPO_SEM_DUPLICATE_NAMED_ARG) => "duplicate named argument in call",
            (Locale::PtBr, Self::AIPO_SEM_DUPLICATE_NAMED_ARG) => {
                "argumento nomeado duplicado na chamada"
            }
            (Locale::En, Self::AIPO_SEM_RETURN_VALUE_MISMATCH) => {
                "return value consistency mismatch"
            }
            (Locale::PtBr, Self::AIPO_SEM_RETURN_VALUE_MISMATCH) => {
                "inconsistência no retorno de valores"
            }
            (Locale::En, Self::AIPO_SEM_PATH_MISSING_RETURN_VALUE) => {
                "code path ends without returning a value"
            }
            (Locale::PtBr, Self::AIPO_SEM_PATH_MISSING_RETURN_VALUE) => {
                "caminho de execução termina sem retornar valor"
            }
            (Locale::En, Self::AIPO_SEM_NON_BOOL_CONDITION) => "condition must evaluate to a Bool",
            (Locale::PtBr, Self::AIPO_SEM_NON_BOOL_CONDITION) => {
                "condição deve resultar em um Bool"
            }
            (Locale::En, Self::AIPO_SEM_NON_EXHAUSTIVE_MATCH) => {
                "non-exhaustive match pattern coverage"
            }
            (Locale::PtBr, Self::AIPO_SEM_NON_EXHAUSTIVE_MATCH) => {
                "padrões de correspondência não cobrem todos os casos"
            }
            (Locale::En, Self::AIPO_SEM_IMPORT_CYCLE) => "circular import dependency detected",
            (Locale::PtBr, Self::AIPO_SEM_IMPORT_CYCLE) => {
                "dependência circular de importação detectada"
            }
            (Locale::En, Self::AIPO_SEM_UNKNOWN_MODULE) => "module path could not be resolved",
            (Locale::PtBr, Self::AIPO_SEM_UNKNOWN_MODULE) => {
                "caminho de módulo não pôde ser resolvido"
            }
            (Locale::En, Self::AIPO_SEM_EXPORT_UNKNOWN) => "exported symbol not declared in module",
            (Locale::PtBr, Self::AIPO_SEM_EXPORT_UNKNOWN) => {
                "símbolo exportado não foi declarado no módulo"
            }
            (Locale::En, Self::AIPO_SEM_CONTRACT_VIOLATION_STATIC) => {
                "static contract violation detected"
            }
            (Locale::PtBr, Self::AIPO_SEM_CONTRACT_VIOLATION_STATIC) => {
                "violação estática de contrato detectada"
            }
            (Locale::En, Self::AIPO_SEM_AWAIT_IN_SUBEXPRESSION) => {
                "await expression not allowed in this position"
            }
            (Locale::PtBr, Self::AIPO_SEM_AWAIT_IN_SUBEXPRESSION) => {
                "expressão await não permitida nesta posição"
            }
            (Locale::En, Self::AIPO_SEM_FORGOTTEN_TASK) => {
                "asynchronous task created but never awaited"
            }
            (Locale::PtBr, Self::AIPO_SEM_FORGOTTEN_TASK) => {
                "tarefa assíncrona criada mas nunca aguardada"
            }
            (Locale::En, Self::AIPO_SEM_NESTED_AWAIT_DO) => {
                "nested await-do blocks are not permitted"
            }
            (Locale::PtBr, Self::AIPO_SEM_NESTED_AWAIT_DO) => {
                "blocos await-do aninhados não são permitidos"
            }
            (Locale::En, Self::AIPO_SEM_PARAMETRIC_CONTRACT) => {
                "parametric contracts are not supported"
            }
            (Locale::PtBr, Self::AIPO_SEM_PARAMETRIC_CONTRACT) => {
                "contratos paramétricos não são suportados"
            }
            (Locale::En, Self::AIPO_SEM_DEPRECATED) => "deprecated item use",
            (Locale::PtBr, Self::AIPO_SEM_DEPRECATED) => "uso de item depreciado",
            (Locale::En, Self::AIPO_SEM_TODO) => "pending todo item",
            (Locale::PtBr, Self::AIPO_SEM_TODO) => "item de desenvolvimento pendente",
            (Locale::En, Self::AIPO_SEM_NOME_DE_HOOK) => "method name resembles a lifecycle hook",
            (Locale::PtBr, Self::AIPO_SEM_NOME_DE_HOOK) => {
                "nome de método se assemelha a um hook de ciclo de vida"
            }
            (Locale::En, Self::AIPO_SEM_HOOK_VAZIO) => {
                "invariant hook has an empty body without conditions"
            }
            (Locale::PtBr, Self::AIPO_SEM_HOOK_VAZIO) => {
                "hook de invariante com corpo vazio sem condições"
            }
            (Locale::En, Self::AIPO_SEM_HOOK_DUPLICADO) => {
                "lifecycle hook is declared more than once on type"
            }
            (Locale::PtBr, Self::AIPO_SEM_HOOK_DUPLICADO) => {
                "hook de ciclo de vida declarado mais de uma vez no tipo"
            }

            // Package
            (Locale::En, Self::AIPO_PKG_RESOLUTION) => {
                "package manifest, dependency graph, or capability resolution failed"
            }
            (Locale::PtBr, Self::AIPO_PKG_RESOLUTION) => {
                "falha na resolução de manifesto, grafo de dependências ou capacidades"
            }
            (Locale::En, Self::AIPO_PKG_LOCK_STALE) => {
                "package lockfile is missing, invalid, or stale"
            }
            (Locale::PtBr, Self::AIPO_PKG_LOCK_STALE) => {
                "arquivo de bloqueio (lockfile) ausente, inválido ou desatualizado"
            }
            (Locale::En, Self::AIPO_PKG_FETCH) => "package artifact fetch failed or corrupted",
            (Locale::PtBr, Self::AIPO_PKG_FETCH) => {
                "falha no download ou artefato de pacote corrompido"
            }

            // Runtime Faults
            (Locale::En, Self::AIPO_RT_OVERFLOW) => "integer arithmetic exceeded range",
            (Locale::PtBr, Self::AIPO_RT_OVERFLOW) => {
                "estouro aritmético de inteiro fora do intervalo"
            }
            (Locale::En, Self::AIPO_RT_NON_FINITE_FLOAT) => {
                "floating-point operation produced NaN or Infinity"
            }
            (Locale::PtBr, Self::AIPO_RT_NON_FINITE_FLOAT) => {
                "operação de ponto flutuante resultou em NaN ou Infinito"
            }
            (Locale::En, Self::AIPO_RT_DIV_ZERO) => "division by zero",
            (Locale::PtBr, Self::AIPO_RT_DIV_ZERO) => "divisão por zero",
            (Locale::En, Self::AIPO_RT_INDEX_OUT_OF_RANGE) => "index out of range",
            (Locale::PtBr, Self::AIPO_RT_INDEX_OUT_OF_RANGE) => "índice fora do intervalo",
            (Locale::En, Self::AIPO_RT_KEY_NOT_FOUND) => "dictionary key not found",
            (Locale::PtBr, Self::AIPO_RT_KEY_NOT_FOUND) => "chave não encontrada no dicionário",
            (Locale::En, Self::AIPO_RT_NOT_CALLABLE) => "invoked value is not callable",
            (Locale::PtBr, Self::AIPO_RT_NOT_CALLABLE) => "valor invocado não é chamável",
            (Locale::En, Self::AIPO_RT_MUTATION_DURING_ITERATION) => {
                "collection mutated during iteration"
            }
            (Locale::PtBr, Self::AIPO_RT_MUTATION_DURING_ITERATION) => {
                "coleção modificada durante iteração"
            }
            (Locale::En, Self::AIPO_RT_TYPE_MISMATCH) => "runtime type mismatch",
            (Locale::PtBr, Self::AIPO_RT_TYPE_MISMATCH) => {
                "incompatibilidade de tipo em tempo de execução"
            }
            (Locale::En, Self::AIPO_RT_CANCELLED) => "cancelled task driven or awaited",
            (Locale::PtBr, Self::AIPO_RT_CANCELLED) => {
                "tarefa cancelada foi executada ou aguardada"
            }
            (Locale::En, Self::AIPO_RT_AWAIT_CYCLE) => {
                "deadlock / cyclic await dependency detected"
            }
            (Locale::PtBr, Self::AIPO_RT_AWAIT_CYCLE) => {
                "dependência cíclica de espera (deadlock) detectada"
            }
            (Locale::En, Self::AIPO_RT_AWAIT_IN_CALLBACK) => {
                "blocking operation inside synchronous callback"
            }
            (Locale::PtBr, Self::AIPO_RT_AWAIT_IN_CALLBACK) => {
                "operação bloqueante dentro de callback síncrono"
            }
            (Locale::En, Self::AIPO_RT_CAPABILITY_DENIED) => {
                "host operation attempted without capability"
            }
            (Locale::PtBr, Self::AIPO_RT_CAPABILITY_DENIED) => {
                "operação do hospedeiro tentada sem capacidade"
            }
            (Locale::En, Self::AIPO_RT_STALE_HANDLE) => {
                "host handle addressed after slot was released"
            }
            (Locale::PtBr, Self::AIPO_RT_STALE_HANDLE) => {
                "manipulador do hospedeiro acessado após liberação"
            }
            (Locale::En, Self::AIPO_RT_SCOPE_ESCAPE) => "scoped host binding escaped to heap",
            (Locale::PtBr, Self::AIPO_RT_SCOPE_ESCAPE) => {
                "vínculo com escopo do hospedeiro escapou para a heap"
            }
            (Locale::En, Self::AIPO_RT_MODULE_INIT_FAILED) => "module top-level evaluation failed",
            (Locale::PtBr, Self::AIPO_RT_MODULE_INIT_FAILED) => {
                "avaliação de topo do módulo falhou"
            }
            (Locale::En, Self::AIPO_RT_INVALID_SCHEMA) => {
                "host surface description is internally inconsistent"
            }
            (Locale::PtBr, Self::AIPO_RT_INVALID_SCHEMA) => {
                "descrição da superfície do hospedeiro inconsistente"
            }

            // Runtime Failure
            (Locale::En, Self::AIPO_RT_FAILURE_UNCAUGHT) => {
                "unhandled failure escaped to top level"
            }
            (Locale::PtBr, Self::AIPO_RT_FAILURE_UNCAUGHT) => {
                "falha não tratada escapou ao nível superior"
            }
        }
    }
}
