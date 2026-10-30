// What a page shows: its view settings (`ui`) and what they resolve to. `describe` runs first
// because resolving may rescope the view (see ViewState.scope).

export async function currentView(panel, page = panel.route.page) {
    const { feature, state, view } = panel.page(page);
    const described = feature.describe ? await feature.describe(state) : {};
    return { page, route: panel.routeOf(page), ui: view.get(), ...described };
}