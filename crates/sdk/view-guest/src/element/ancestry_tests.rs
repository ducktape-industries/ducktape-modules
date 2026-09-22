use super::*;
use crate::InteractiveElement;
use std::cell::RefCell;
use std::rc::Rc;

struct PathProbe {
    id: Option<ElementId>,
    paths: Rc<RefCell<Vec<Vec<wire::ElementIdWire>>>>,
}

impl PathProbe {
    fn identified(
        id: impl Into<ElementId>,
        paths: &Rc<RefCell<Vec<Vec<wire::ElementIdWire>>>>,
    ) -> Self {
        Self {
            id: Some(id.into()),
            paths: paths.clone(),
        }
    }

    fn anonymous(paths: &Rc<RefCell<Vec<Vec<wire::ElementIdWire>>>>) -> Self {
        Self {
            id: None,
            paths: paths.clone(),
        }
    }
}

impl IntoElement for PathProbe {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for PathProbe {
    fn id(&self) -> Option<ElementId> {
        self.id.clone()
    }

    fn lower(self: Box<Self>, lowering: &mut Lowering<'_>) -> wire::Node {
        self.paths
            .borrow_mut()
            .push(lowering.current_path().to_vec());
        wire::Node::Text {
            id: self
                .id
                .map(|id| wire::ElementIdWire::from_gpui(id).unwrap()),
            style: StyleRefinement::default(),
            content: "probe".into(),
            heading: None,
            live: None,
        }
    }
}

struct ProbeComponent {
    paths: Rc<RefCell<Vec<Vec<wire::ElementIdWire>>>>,
}

impl RenderOnce for ProbeComponent {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        div().child(PathProbe::identified("leaf", &self.paths).into_any_element())
    }
}

impl IntoElement for ProbeComponent {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for ProbeComponent {
    fn lower(self: Box<Self>, lowering: &mut Lowering<'_>) -> wire::Node {
        lowering.render_once(*self)
    }
}

fn lower(element: impl IntoElement) -> wire::Node {
    let mut app = App::new(false);
    let mut window = app.window();
    Lowering::new(&mut window, &mut app).lower(element)
}

fn named(name: &'static str) -> wire::ElementIdWire {
    wire_id(ElementId::Name(name.into()))
}

fn wire_id(id: ElementId) -> wire::ElementIdWire {
    wire::ElementIdWire::from_gpui(id).unwrap()
}

#[test]
fn equal_local_ids_have_distinct_typed_parent_paths() {
    let paths = Rc::new(RefCell::new(Vec::new()));
    lower(
        div()
            .child(
                div()
                    .id(ElementId::Integer(1))
                    .child(PathProbe::identified("same", &paths)),
            )
            .child(
                div()
                    .id(ElementId::Name("1".into()))
                    .child(PathProbe::identified("same", &paths)),
            ),
    );
    assert_eq!(
        *paths.borrow(),
        vec![
            vec![wire_id(ElementId::Integer(1)), named("same")],
            vec![wire_id(ElementId::Name("1".into())), named("same")]
        ]
    );
}

#[test]
fn anonymous_any_and_render_once_wrappers_are_transparent() {
    let paths = Rc::new(RefCell::new(Vec::new()));
    lower(div().id("root").child(div().child(ProbeComponent {
        paths: paths.clone(),
    })));
    assert_eq!(*paths.borrow(), vec![vec![named("root"), named("leaf")]]);
}

#[test]
fn identified_child_scope_is_popped_before_its_sibling() {
    let paths = Rc::new(RefCell::new(Vec::new()));
    lower(
        div()
            .id("root")
            .child(div().id("branch").child(PathProbe::anonymous(&paths)))
            .child(PathProbe::anonymous(&paths)),
    );
    assert_eq!(
        *paths.borrow(),
        vec![vec![named("root"), named("branch")], vec![named("root")]]
    );
}
