//! Port of `packages/pyright-internal/src/parser/stringAnnotationInfo.ts`.
//!
//! In TypeScript, `createStringAnnotationInfo` returns two objects whose
//! closures share one `Map`. Here both are handles to one `StringAnnotationMap`
//! in the arena: `StringAnnotationInfo` reads it, `StringAnnotationInfoWriter`
//! writes it. Handles compare by identity, like the TypeScript objects.

use std::cell::RefCell;

use jsrt::{JsMap, P};

use super::parse_nodes::{ExpressionNode, StringListNode};

#[derive(jsrt::Trace)]
pub struct StringAnnotationMap {
    annotations: RefCell<JsMap<StringListNode, ExpressionNode>>,
}

// parser/stringAnnotationInfo.ts:3
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[derive(jsrt::Trace)]
pub struct StringAnnotationInfo(P<StringAnnotationMap>);

impl StringAnnotationInfo {
    pub fn size(&self) -> i32 {
        self.0.annotations.borrow().len() as i32
    }

    pub fn get(&self, node: StringListNode) -> Option<ExpressionNode> {
        self.0.annotations.borrow().get(&node).copied()
    }

    /// `Map.forEach`: visits entries in insertion order, including entries the
    /// callback adds.
    pub fn for_each(&self, mut callback: impl FnMut(ExpressionNode, StringListNode)) {
        let mut i = 0;
        loop {
            let entry = self
                .0
                .annotations
                .borrow()
                .get_index(i)
                .map(|(&node, &annotation)| (annotation, node));
            let Some((annotation, node)) = entry else {
                break;
            };
            callback(annotation, node);
            i += 1;
        }
    }
}

// parser/stringAnnotationInfo.ts:9
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StringAnnotationInfoWriter(P<StringAnnotationMap>);

impl StringAnnotationInfoWriter {
    pub fn set(&self, node: StringListNode, annotation: ExpressionNode) {
        self.0.annotations.borrow_mut().insert(node, annotation);
    }

    pub fn add_all(&self, info: StringAnnotationInfo) {
        info.for_each(|annotation, node| self.set(node, annotation));
    }
}

pub struct StringAnnotationInfoAndWriter {
    pub info: StringAnnotationInfo,
    pub writer: StringAnnotationInfoWriter,
}

// parser/stringAnnotationInfo.ts:14
pub fn create_string_annotation_info() -> StringAnnotationInfoAndWriter {
    let annotations = P::new(StringAnnotationMap {
        annotations: RefCell::new(JsMap::default()),
    });

    StringAnnotationInfoAndWriter {
        info: StringAnnotationInfo(annotations),
        writer: StringAnnotationInfoWriter(annotations),
    }
}

thread_local! {
    static EMPTY_STRING_ANNOTATION_INFO: StringAnnotationInfo = StringAnnotationInfo(P::new(StringAnnotationMap {
        annotations: RefCell::new(JsMap::default()),
    }));
}

/// The frozen empty info. No writer exists for it, so it stays empty.
// parser/stringAnnotationInfo.ts:35
pub fn empty_string_annotation_info() -> StringAnnotationInfo {
    EMPTY_STRING_ANNOTATION_INFO.with(|info| *info)
}
