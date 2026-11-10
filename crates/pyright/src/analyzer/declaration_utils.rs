//! Port of `packages/pyright-internal/src/analyzer/declarationUtils.ts`.
//!
//! Only the functions `symbol.ts` calls are ported so far. The rest belongs to
//! the evaluator step.

use super::declaration::{Declaration, DeclarationType};
use crate::parser::parse_nodes::{FunctionNode, ParameterNode};

// analyzer/declarationUtils.ts:27
pub fn has_type_for_declaration(declaration: &Declaration) -> bool {
    match declaration.type_ {
        DeclarationType::Intrinsic
        | DeclarationType::Class
        | DeclarationType::SpecialBuiltInClass
        | DeclarationType::Function
        | DeclarationType::TypeParam
        | DeclarationType::TypeAlias => true,

        DeclarationType::Param => {
            let node = ParameterNode::cast(declaration.node.get()).unwrap();
            if node.d().annotation.get().is_some() || node.d().annotation_comment.get().is_some() {
                return true;
            }

            // Handle function type comments.
            let parameter_parent = node.parent.get().and_then(FunctionNode::cast);
            if let Some(parameter_parent) = parameter_parent {
                if let Some(func_annotation_comment) = parameter_parent.d().func_annotation_comment.get() {
                    if !func_annotation_comment.d().is_ellipsis.get() {
                        let param_annotations_len = func_annotation_comment.d().param_annotations.borrow().len();

                        // Handle the case where the annotation comment is missing an
                        // annotation for the first parameter (self or cls).
                        let params = parameter_parent.d().params.borrow();
                        if params.len() > param_annotations_len && node == params[0].node() {
                            return false;
                        }

                        return true;
                    }
                }
            }
            false
        }

        DeclarationType::Variable => declaration.variable().unwrap().type_annotation_node.get().is_some(),

        DeclarationType::Alias => false,
    }
}

// analyzer/declarationUtils.ts:71
pub fn are_declarations_same(
    decl1: &Declaration,
    decl2: &Declaration,
    treat_module_in_import_and_from_import_same: bool,
    skip_range_for_aliases: bool,
) -> bool {
    if decl1.type_ != decl2.type_ {
        return false;
    }

    if !decl1.uri.get().equals(Some(decl2.uri.get())) {
        return false;
    }

    if !skip_range_for_aliases || decl1.type_ != DeclarationType::Alias {
        let range1 = decl1.range.get();
        let range2 = decl2.range.get();
        if range1.start.line != range2.start.line || range1.start.character != range2.start.character {
            return false;
        }
    }

    // Alias declarations refer to the entire import statement.
    // We need to further differentiate.
    if let (Some(alias1), Some(alias2)) = (decl1.alias(), decl2.alias()) {
        if *alias1.symbol_name.borrow() != *alias2.symbol_name.borrow()
            || alias1.uses_local_name.get() != alias2.uses_local_name.get()
        {
            return false;
        }

        if treat_module_in_import_and_from_import_same {
            // Treat "module" in "import [|module|]", "from [|module|] import ..."
            // or "from ... import [|module|]" same in IDE services.
            //
            // Some case such as "from [|module|] import ...", symbol for [|module|] doesn't even
            // exist and it can't be referenced inside of a module, but nonetheless, IDE still
            // needs these sometimes for things like hover tooltip, highlight references,
            // find all references and etc.
            return true;
        }

        if decl1.node.get() != decl2.node.get() {
            return false;
        }
    }

    true
}
