//! An HTML5 fragment parsed by html5ever into a flat node arena: what the
//! sanitizer walks (`specs/text/description-html.md` D3). Nodes refer to each
//! other by index, so the tree needs no reference counting and dropping it
//! never recurses.

use std::borrow::Cow;
use std::cell::{Ref, RefCell};

use html5ever::tendril::{StrTendril, TendrilSink};
use html5ever::tree_builder::{ElementFlags, NodeOrText, QuirksMode, TreeSink};
use html5ever::{Attribute, LocalName, Namespace, ParseOpts, QualName, parse_fragment};

pub(super) const HTML_NAMESPACE: &str = "http://www.w3.org/1999/xhtml";

/// Index of a node in [`Fragment::nodes`]; the document is 0.
pub(super) type NodeId = usize;

pub(super) enum NodeData {
    Document,
    Element {
        name: QualName,
        attrs: Vec<Attribute>,
    },
    Text(String),
    /// Comments, processing instructions, doctypes and template contents:
    /// never written out.
    Ignored,
}

pub(super) struct Node {
    pub data: NodeData,
    pub children: Vec<NodeId>,
    parent: Option<NodeId>,
    /// A `template` element's contents, kept apart from its children.
    template_contents: Option<NodeId>,
    /// A MathML `annotation-xml` element whose `encoding` is HTML: an HTML
    /// integration point, so HTML start tags inside it stay inside it
    /// instead of closing the `math` element.
    html_integration_point: bool,
}

/// A parsed fragment: the document node, an `html` element under it, and the
/// fragment's nodes under that.
pub(super) struct Fragment {
    pub nodes: Vec<Node>,
}

impl Fragment {
    /// Parses `html` as the content of a `body` element.
    pub fn parse(html: &str) -> Fragment {
        let context = QualName::new(None, Namespace::from(HTML_NAMESPACE), LocalName::from("body"));
        let sink = Sink {
            nodes: RefCell::new(vec![Node::new(NodeData::Document)]),
        };
        let parser = parse_fragment(sink, ParseOpts::default(), context, Vec::new(), false);
        parser.one(StrTendril::from_slice(html))
    }
}

impl Node {
    fn new(data: NodeData) -> Node {
        Node {
            data,
            children: Vec::new(),
            parent: None,
            template_contents: None,
            html_integration_point: false,
        }
    }
}

struct Sink {
    nodes: RefCell<Vec<Node>>,
}

impl Sink {
    fn push(&self, node: Node) -> NodeId {
        let mut nodes = self.nodes.borrow_mut();
        nodes.push(node);
        nodes.len() - 1
    }

    fn detach(&self, id: NodeId) {
        let mut nodes = self.nodes.borrow_mut();
        if let Some(parent) = nodes[id].parent.take() {
            nodes[parent].children.retain(|&child| child != id);
        }
    }

    /// Inserts `child`, already detached, into `parent` at `index`, merging
    /// text into a text node just before that position.
    fn insert(&self, parent: NodeId, index: usize, child: NodeOrText<NodeId>) {
        match child {
            NodeOrText::AppendText(text) => {
                let mut nodes = self.nodes.borrow_mut();
                let before = index.checked_sub(1).map(|i| nodes[parent].children[i]);
                if let Some(before) = before
                    && let NodeData::Text(existing) = &mut nodes[before].data
                {
                    existing.push_str(&text);
                    return;
                }
                nodes.push(Node {
                    parent: Some(parent),
                    ..Node::new(NodeData::Text(text.to_string()))
                });
                let id = nodes.len() - 1;
                nodes[parent].children.insert(index, id);
            }
            NodeOrText::AppendNode(id) => {
                let mut nodes = self.nodes.borrow_mut();
                nodes[parent].children.insert(index, id);
                nodes[id].parent = Some(parent);
            }
        }
    }

    fn position(&self, id: NodeId) -> Option<(NodeId, usize)> {
        let nodes = self.nodes.borrow();
        let parent = nodes[id].parent?;
        let index = nodes[parent].children.iter().position(|&child| child == id)?;
        Some((parent, index))
    }
}

impl TreeSink for Sink {
    type Handle = NodeId;
    type Output = Fragment;
    type ElemName<'a>
        = Ref<'a, QualName>
    where
        Self: 'a;

    fn finish(self) -> Fragment {
        Fragment {
            nodes: self.nodes.into_inner(),
        }
    }

    fn parse_error(&self, _message: Cow<'static, str>) {}

    fn get_document(&self) -> NodeId {
        0
    }

    fn elem_name<'a>(&'a self, target: &'a NodeId) -> Ref<'a, QualName> {
        Ref::map(self.nodes.borrow(), |nodes| match &nodes[*target].data {
            NodeData::Element { name, .. } => name,
            _ => unreachable!("html5ever asks for the name of elements only"),
        })
    }

    fn create_element(&self, name: QualName, attrs: Vec<Attribute>, flags: ElementFlags) -> NodeId {
        let template_contents = flags.template.then(|| self.push(Node::new(NodeData::Ignored)));
        self.push(Node {
            template_contents,
            html_integration_point: flags.mathml_annotation_xml_integration_point,
            ..Node::new(NodeData::Element { name, attrs })
        })
    }

    fn is_mathml_annotation_xml_integration_point(&self, handle: &NodeId) -> bool {
        self.nodes.borrow()[*handle].html_integration_point
    }

    fn create_comment(&self, _text: StrTendril) -> NodeId {
        self.push(Node::new(NodeData::Ignored))
    }

    fn create_pi(&self, _target: StrTendril, _data: StrTendril) -> NodeId {
        self.push(Node::new(NodeData::Ignored))
    }

    fn append(&self, parent: &NodeId, child: NodeOrText<NodeId>) {
        if let NodeOrText::AppendNode(id) = &child {
            self.detach(*id);
        }
        let end = self.nodes.borrow()[*parent].children.len();
        self.insert(*parent, end, child);
    }

    fn append_based_on_parent_node(&self, element: &NodeId, prev_element: &NodeId, child: NodeOrText<NodeId>) {
        if self.nodes.borrow()[*element].parent.is_some() {
            self.append_before_sibling(element, child);
        } else {
            self.append(prev_element, child);
        }
    }

    fn append_doctype_to_document(&self, _name: StrTendril, _public_id: StrTendril, _system_id: StrTendril) {}

    fn get_template_contents(&self, target: &NodeId) -> NodeId {
        self.nodes.borrow()[*target]
            .template_contents
            .expect("html5ever asks for the contents of template elements only")
    }

    fn same_node(&self, x: &NodeId, y: &NodeId) -> bool {
        x == y
    }

    fn set_quirks_mode(&self, _mode: QuirksMode) {}

    fn append_before_sibling(&self, sibling: &NodeId, new_node: NodeOrText<NodeId>) {
        // Detach first: a node moving from earlier in the same parent would
        // otherwise shift the sibling's position.
        if let NodeOrText::AppendNode(id) = &new_node {
            self.detach(*id);
        }
        if let Some((parent, index)) = self.position(*sibling) {
            self.insert(parent, index, new_node);
        }
    }

    fn add_attrs_if_missing(&self, target: &NodeId, attrs: Vec<Attribute>) {
        if let NodeData::Element { attrs: existing, .. } = &mut self.nodes.borrow_mut()[*target].data {
            for attr in attrs {
                if !existing.iter().any(|e| e.name == attr.name) {
                    existing.push(attr);
                }
            }
        }
    }

    fn remove_from_parent(&self, target: &NodeId) {
        self.detach(*target);
    }

    fn reparent_children(&self, node: &NodeId, new_parent: &NodeId) {
        let mut nodes = self.nodes.borrow_mut();
        let moved = std::mem::take(&mut nodes[*node].children);
        for &child in &moved {
            nodes[child].parent = Some(*new_parent);
        }
        nodes[*new_parent].children.extend(moved);
    }
}
