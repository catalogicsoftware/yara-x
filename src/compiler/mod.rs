/*! Compiles YARA source code into binary form.

YARA rules must be compiled before they can be used for scanning data. This
module implements the YARA compiler.
*/
use std::cell::RefCell;
use std::path::Path;
use std::sync::Arc;
use std::{fmt, mem};
use walrus::ir::{InstrSeqId, UnaryOp};
use walrus::{Module, ValType};

use crate::ast::*;
use crate::compiler::emit::{catch_undef, emit_bool_expr};
use crate::compiler::semcheck::{semcheck, warning_if_not_boolean};
use crate::parser::{ErrorInfo as ParserError, Parser, SourceCode};
use crate::report::ReportBuilder;
use crate::string_pool::{BStringPool, StringPool};
use crate::types::Type;
use crate::warnings::Warning;
use crate::wasm::{ModuleBuilder, WasmSymbols};

#[doc(inline)]
pub use crate::compiler::errors::*;
use crate::symbols::{StackedSymbolTable, Symbol, SymbolLookup, SymbolTable};

mod emit;
mod errors;
mod semcheck;

#[cfg(test)]
mod tests;

/// A YARA compiler.
pub struct Compiler<'a> {
    colorize_errors: bool,

    report_builder: ReportBuilder,
    symbol_table: StackedSymbolTable<'a>,
    /// Pool that contains all the identifiers used in the rules. Each
    /// identifier appears only once, even if they are used by multiple
    /// rules. For example, the pool contains a single copy of the common
    /// identifier `$a`. Identifiers have an unique 32-bits IDs ([`IdentId`])
    /// that can be used for retrieving them from the pool.
    ident_pool: StringPool<IdentId>,

    /// Similar to `ident_pool` but for string literals found in the source
    /// code. As literal strings in YARA can contain arbitrary bytes, a pool
    /// pool capable of storing [`bstr::BString`] must be used, the [`String`]
    /// type only accepts valid UTF-8.
    lit_pool: BStringPool<LiteralId>,

    /// Builder for creating the WebAssembly module that contains the code
    /// for all rule conditions.
    wasm_mod: ModuleBuilder,

    /// A vector with all the rules that has been compiled. A [`RuleId`] is
    /// an index in this vector.
    rules: Vec<CompiledRule>,

    /// A vector with all the patterns from all the rules. A [`PatternId`]
    /// is an index in this vector.
    patterns: Vec<Pattern>,

    /// Vector with the names of all the imported modules. The vector contains
    /// the [`IdentId`] corresponding to the module's identifier.
    imported_modules: Vec<IdentId>,

    /// Warnings generated while compiling the rules.
    warnings: Vec<Warning>,
}

impl<'a> Compiler<'a> {
    /// Creates a new YARA compiler.
    pub fn new() -> Self {
        Self {
            colorize_errors: false,
            warnings: Vec::new(),
            rules: Vec::new(),
            patterns: Vec::new(),
            imported_modules: Vec::new(),
            report_builder: ReportBuilder::new(),
            ident_pool: StringPool::new(),
            lit_pool: BStringPool::new(),
            wasm_mod: ModuleBuilder::new(),
            symbol_table: StackedSymbolTable::new(),
        }
    }

    /// Specifies whether the compiler should produce colorful error messages.
    ///
    /// Colorized error messages contain ANSI escape sequences that make them
    /// look nicer on compatible consoles. The default setting is `false`.
    pub fn colorize_errors(mut self, b: bool) -> Self {
        self.colorize_errors = b;
        self
    }

    /// Adds a YARA source code to be compiled.
    ///
    /// This function can be called multiple times.
    pub fn add_source<'src, S>(mut self, src: S) -> Result<Self, Error>
    where
        S: Into<SourceCode<'src>>,
    {
        self.report_builder.with_colors(self.colorize_errors);

        let src = src.into();

        let mut ast = Parser::new()
            .set_report_builder(&self.report_builder)
            .build_ast(src.clone())?;

        // Transfer to the compiler the warnings generated by the parser.
        self.warnings.append(&mut ast.warnings);

        for ns in ast.namespaces.iter_mut() {
            // Process import statements. Checks that all imported modules
            // actually exist, and raise warnings in case of duplicated
            // imports.
            self.process_imports(&src, &ns.imports)?;

            // Iterate over the list of declared rules.
            for rule in ns.rules.iter_mut() {
                // Create array with pairs (IdentID, PatternID) that describe
                // the patterns in a compiled rule.
                let pairs = if let Some(patterns) = &rule.patterns {
                    let mut pairs = Vec::with_capacity(patterns.len());
                    for pattern in patterns {
                        let ident_id = self
                            .ident_pool
                            .get_or_intern(pattern.identifier().as_str());

                        // The PatternID is the index of the pattern in
                        // `self.patterns`.
                        let pattern_id = self.patterns.len() as PatternId;

                        self.patterns.push(Pattern {});

                        pairs.push((ident_id, pattern_id));
                    }
                    pairs
                } else {
                    Vec::new()
                };

                let rule_id = self.rules.len() as RuleId;

                self.rules.push(CompiledRule {
                    ident: self
                        .ident_pool
                        .get_or_intern(rule.identifier.as_str()),
                    patterns: pairs,
                });

                let mut ctx = Context {
                    src: &src,
                    current_struct: None,
                    symbol_table: &mut self.symbol_table,
                    ident_pool: &mut self.ident_pool,
                    lit_pool: &mut self.lit_pool,
                    report_builder: &self.report_builder,
                    current_rule: self.rules.last().unwrap(),
                    wasm_symbols: self.wasm_mod.wasm_symbols(),
                    warnings: &mut self.warnings,
                    exception_handler_stack: Vec::new(),
                    stack_top: 0,
                };

                // Verify that the rule's condition is semantically valid. This
                // traverses the condition's AST recursively. The condition can
                // be an expression returning a bool, integer, float or string.
                // Integer, float and string result are casted to boolean.
                semcheck!(
                    &mut ctx,
                    Type::Bool | Type::Integer | Type::Float | Type::String,
                    &mut rule.condition
                )?;

                // However, if the condition's result is not a boolean and must
                // be casted, raise a warning about it.
                warning_if_not_boolean(&mut ctx, &rule.condition);

                // TODO: add rule name to declared identifiers.

                let ctx = RefCell::new(ctx);

                self.wasm_mod.main_fn().block(None, |block| {
                    catch_undef(&ctx, block, |instr| {
                        emit_bool_expr(&ctx, instr, &rule.condition);
                    });

                    // If the condition's result is 0, jump out of the block
                    // and don't call the `rule_result` function.
                    block.unop(UnaryOp::I32Eqz);
                    block.br_if(block.id());

                    // The RuleID is the argument to `rule_match`.
                    block.i32_const(rule_id);

                    // Emit call instruction for calling `rule_match`.
                    block.call(ctx.borrow().wasm_symbols.rule_match);
                });

                // After emitting the whole condition, the stack should be empty.
                assert_eq!(ctx.borrow().stack_top, 0);
            }
        }

        Ok(self)
    }

    /// Builds the source code previously added to the compiler.
    ///
    /// This function consumes the compiler and returns an instance of
    /// [`CompiledRules`].
    pub fn build(self) -> Result<CompiledRules, Error> {
        // Finish building the WebAssembly module.
        let mut wasm_mod = self.wasm_mod.build();

        // Compile the WebAssembly module for the current platform. This
        // panics if the WebAssembly code is somehow invalid, which should
        // not happen, as the code is generated by YARA itself.
        let compiled_wasm_mod = wasmtime::Module::from_binary(
            &crate::wasm::ENGINE,
            wasm_mod.emit_wasm().as_slice(),
        )
        .unwrap();

        Ok(CompiledRules {
            compiled_wasm_mod,
            wasm_mod,
            ident_pool: self.ident_pool,
            lit_pool: self.lit_pool,
            imported_modules: self.imported_modules,
            patterns: Vec::new(),
            rules: self.rules,
        })
    }

    /// Emits a `.wasm` file with the WebAssembly module generated for the
    /// rules.
    ///
    /// When YARA rules are compiled their conditions are translated to
    /// WebAssembly. This function emits the WebAssembly module that contains
    /// the code produced for these rules. The module can be inspected or
    /// disassembled with third-party [tooling](https://github.com/WebAssembly/wabt).
    pub fn emit_wasm_file<P>(self, path: P) -> Result<(), Error>
    where
        P: AsRef<Path>,
    {
        // Finish building the WebAssembly module.
        let mut wasm_mod = self.wasm_mod.build();
        Ok(wasm_mod.emit_wasm_file(path)?)
    }
}

impl<'a> Compiler<'a> {
    fn process_imports(
        &mut self,
        src: &SourceCode,
        imports: &[Import],
    ) -> Result<(), Error> {
        let mut symbol_table = SymbolTable::new();

        // Iterate over the list of imported modules.
        for import in imports.iter() {
            // Does the imported module actually exist? ...
            if let Some(module) = crate::modules::BUILTIN_MODULES
                .get(import.module_name.as_str())
            {
                // ... if yes, add the module to the list of imported modules
                // and the symbol table.
                self.imported_modules.push(
                    self.ident_pool.get_or_intern(import.module_name.as_str()),
                );

                symbol_table.insert(
                    import.module_name.as_str(),
                    Symbol::new_struct(Arc::new(module)),
                );
            } else {
                // ... if no, that's an error.
                return Err(Error::CompileError(
                    CompileError::unknown_module(
                        &self.report_builder,
                        src,
                        import.module_name.to_string(),
                        import.span(),
                    ),
                ));
            }
        }

        self.symbol_table.push(Arc::new(symbol_table));

        Ok(())
    }
}

impl fmt::Debug for Compiler<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Compiler")
    }
}

impl Default for Compiler<'_> {
    fn default() -> Self {
        Self::new()
    }
}

/// ID associated to each identifier in the identifiers pool.
#[derive(Copy, Clone)]
pub(crate) struct IdentId(u32);

impl From<u32> for IdentId {
    fn from(v: u32) -> Self {
        Self(v)
    }
}

impl From<IdentId> for u32 {
    fn from(v: IdentId) -> Self {
        v.0
    }
}

/// ID associated to each literal string in the literals pool.
#[derive(Copy, Clone)]
pub(crate) struct LiteralId(u32);

impl From<u32> for LiteralId {
    fn from(v: u32) -> Self {
        Self(v)
    }
}

impl From<LiteralId> for u32 {
    fn from(v: LiteralId) -> Self {
        v.0
    }
}

/// ID associated to each pattern.
pub(crate) type PatternId = i32;

/// ID associated to each rule.
pub(crate) type RuleId = i32;

/// Structure that contains information and data structures required during the
/// the current compilation process.
struct Context<'a, 'sym> {
    /// Builder for creating error and warning reports.
    report_builder: &'a ReportBuilder,

    /// Symbol table that contains the currently defined identifiers, modules,
    /// functions, etc
    symbol_table: &'a mut StackedSymbolTable<'sym>,

    /// Symbol table for the currently active structure. When this is None
    /// symbols are looked up in `symbol_table` instead.
    current_struct: Option<Arc<dyn SymbolLookup<'sym> + 'a>>,

    /// Table with all the symbols (functions, variables) used by WebAssembly
    /// code.
    wasm_symbols: WasmSymbols,

    /// Source code that is being compiled.
    src: &'a SourceCode<'a>,

    /// Rule that is being compiled.
    current_rule: &'a CompiledRule,

    /// Warnings generated during the compilation.
    warnings: &'a mut Vec<Warning>,

    /// Pool with identifiers used in the rules.
    ident_pool: &'a mut StringPool<IdentId>,

    /// Pool with literal strings used in the rules.
    lit_pool: &'a mut BStringPool<LiteralId>,

    /// Stack of installed exception handlers for catching undefined values.
    exception_handler_stack: Vec<(ValType, InstrSeqId)>,

    /// YARA uses a WebAssembly module memory (maybe I should say *the* module
    /// memory, because at this moment a single memory is supported) for
    /// storing loop variables in a stack. This shouldn't be confused with the
    /// WebAssembly runtime stack, where the instructions take their arguments
    /// from and pushes their results back, this is a different stack that
    /// allows having nested loops.
    ///
    /// This value stores the memory offset for the stack's top, it starts with
    /// 0 and grows in increments of 8 bytes, the size of an i64. Memory
    /// offsets from 0 to stack_top are occupied with loop variables.
    stack_top: i32,
}

impl<'a, 'sym> Context<'a, 'sym> {
    /// Given an [`IdentID`] returns the identifier as `&str`.
    ///
    /// Panics if no identifier has the provided [`IdentID`].
    #[inline]
    fn resolve_ident(&self, ident_id: IdentId) -> &str {
        self.ident_pool.get(ident_id).unwrap()
    }

    /// Allocates space for a new variable in the stack, and returns the
    /// memory offset where the new variable resides.
    ///
    /// Do not confuse this stack with the WebAssembly runtime stack, this
    /// stack is stored in the module's memory. This function panics if the
    /// stacks grows beyond the module's memory size.
    #[inline]
    fn new_var(&mut self) -> i32 {
        let top = self.stack_top;
        self.stack_top += mem::size_of::<i64>() as i32;
        if self.stack_top > (ModuleBuilder::MODULE_MEMORY_SIZE * 65536) as i32
        {
            panic!("variables stack grew larger than module's memory size");
        }
        top
    }

    /// Frees stack space previously allocated with [`Context::new_var`]. This
    /// function restores the stop of the stack to the value provided in the
    /// argument, effectively releasing all the stack space after that offset.
    /// for example:
    ///
    /// ```text
    /// let var1 = ctx.new_var()
    /// let var2 = ctx.new_var()
    /// let var3 = ctx.new_var()
    /// // Frees both var2 and var3, because var3 was allocated after var2
    /// ctx.free_vars(var2)
    /// ```
    #[inline]
    fn free_vars(&mut self, top: i32) {
        self.stack_top = top;
    }

    /// Given a pattern identifier (e.g. `$a`) search for it in the current
    /// rule and return its [`PatternID`]. Panics if the current rule does not
    /// have the requested pattern.
    fn get_pattern_from_current_rule(&self, ident: &Ident) -> PatternId {
        for (ident_id, pattern_id) in &self.current_rule.patterns {
            if self.resolve_ident(*ident_id) == ident.as_str() {
                return *pattern_id;
            }
        }
        panic!(
            "rule `{}` does not have pattern `{}` ",
            self.resolve_ident(self.current_rule.ident),
            ident.as_str()
        );
    }
}

/// A set of YARA rules in compiled form.
///
/// This is the result from [`Compiler::build`].
pub struct CompiledRules {
    /// Pool with identifiers used in the rules. Each identifier has its
    /// own [`IdentId`], which can be used for retrieving the identifier
    /// from the pool as a `&str`.
    ident_pool: StringPool<IdentId>,

    /// Pool with literal strings used in the rules. Each literal has its
    /// own [`LiteralId`], which can be used for retrieving the literal
    /// string as `&BStr`.
    lit_pool: BStringPool<LiteralId>,

    /// WebAssembly module containing the code for all rule conditions.
    wasm_mod: Module,

    /// WebAssembly module already compiled into native code for the current
    /// platform.
    compiled_wasm_mod: wasmtime::Module,

    /// Vector with the names of all the imported modules. The vector contains
    /// the [`IdentId`] corresponding to the module's identifier.
    imported_modules: Vec<IdentId>,

    /// Vector containing all the compiled rules. A [`RuleId`] is an index
    /// in this vector.
    rules: Vec<CompiledRule>,

    /// Vector with all the patterns used in the rules. This vector has not
    /// duplicated items, if two different rules use the "MZ" pattern, it
    /// appears in this list once. A [`PatternId`] is an index in this
    /// vector.
    patterns: Vec<Pattern>,
}

impl CompiledRules {
    /// Returns an slice with the individual rules that were compiled.
    #[inline]
    pub fn rules(&self) -> &[CompiledRule] {
        self.rules.as_slice()
    }

    /// An iterator that yields the name of the modules imported by the
    /// rules.
    pub fn imported_modules(&self) -> ImportedModules {
        ImportedModules {
            iter: self.imported_modules.iter(),
            ident_pool: &self.ident_pool,
        }
    }

    #[inline]
    pub(crate) fn lit_pool(&self) -> &BStringPool<LiteralId> {
        &self.lit_pool
    }

    #[inline]
    pub(crate) fn ident_pool(&self) -> &StringPool<IdentId> {
        &self.ident_pool
    }

    #[inline]
    pub(crate) fn compiled_wasm_mod(&self) -> &wasmtime::Module {
        &self.compiled_wasm_mod
    }
}

/// Iterator that returns the modules imported by the rules.
pub struct ImportedModules<'a> {
    iter: std::slice::Iter<'a, IdentId>,
    ident_pool: &'a StringPool<IdentId>,
}

impl<'a> Iterator for ImportedModules<'a> {
    type Item = &'a str;

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next().map(|id| self.ident_pool.get(*id).unwrap())
    }
}

/// Each of the individual rules included in [`CompiledRules`].
pub struct CompiledRule {
    /// The ID of the rule identifier in the identifiers pool.
    pub(crate) ident: IdentId,

    /// Vector with all the patterns defined by this rule.
    patterns: Vec<(IdentId, PatternId)>,
}

/// A pattern in the compiled rules.
struct Pattern {}
