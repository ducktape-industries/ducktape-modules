use super::*;
use crate::ViewElement;
use std::borrow::Cow;

#[derive(crate::IntoElement)]
struct DerivedComponent;

impl RenderOnce for DerivedComponent {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        "derived"
    }
}

#[derive(crate::IntoElement)]
enum DerivedEnum {
    Unit,
}

impl RenderOnce for DerivedEnum {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        "enum"
    }
}

#[derive(crate::IntoElement)]
struct GenericComponent<T>
where
    T: Clone + 'static,
{
    value: T,
}

impl<T: Clone + 'static> RenderOnce for GenericComponent<T> {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let _ = self.value;
        "generic"
    }
}

fn assert_element<T: Element>() {}

fn assert_view_element<T: RenderOnce>(_: ViewElement<T>) {}

#[test]
fn authoring_associated_types_follow_gpui() {
    fn string() -> <String as IntoElement>::Element {
        String::from("string").into_element()
    }
    fn text() -> <&'static str as IntoElement>::Element {
        "text".into_element()
    }
    fn shared() -> <SharedString as IntoElement>::Element {
        SharedString::from("shared").into_element()
    }
    fn borrowed() -> <Cow<'static, str> as IntoElement>::Element {
        Cow::Borrowed("borrowed").into_element()
    }

    assert_element::<SharedString>();
    assert_element::<&'static str>();
    assert_element::<Div>();
    assert_eq!(string().to_string(), "string");
    assert_eq!((*text()).to_owned(), "text");
    assert_eq!(shared().to_string(), "shared");
    assert_eq!(borrowed().to_string(), "borrowed");
}

#[test]
fn derive_and_any_element_use_the_internal_lowering_boundary() {
    let _: AnyElement = DerivedComponent.into_any_element();
    let _: AnyElement = div().child(DerivedComponent).into_any_element();
}

#[test]
fn derive_matches_gpui_component_element_shape() {
    assert_view_element(DerivedComponent.into_element());
    assert_view_element(DerivedEnum::Unit.into_element());
    assert_view_element(GenericComponent { value: 7_u8 }.into_element());
}

#[test]
fn derived_components_and_wrappers_have_fluent_builders() {
    use gpui::prelude::FluentBuilder;

    let _: DerivedComponent = DerivedComponent.when(true, |component| component);
    let _: ViewElement<DerivedComponent> = DerivedComponent
        .into_element()
        .when(true, |element| element);
}
