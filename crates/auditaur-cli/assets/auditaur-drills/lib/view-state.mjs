// Server-owned view state of one page: what it shows (tab, filter, mode), so the agent can read and
// change it with the same effect as the user. Pages render it and write it back through
// `POST /api/<page>/view`; agents use the `get_view` / `set_view` actions. The panel (app-panel.mjs)
// relays `changed()` to its event stream; the HTTP and action wrappers notify after every write.

import { DrillError } from "./errors.mjs";

const invalid = (name, message) => new DrillError("invalid_view", `${name}: ${message}`);

// A field: `schema` describes it to the agent; `check` validates the same value for page writes,
// which bypass the runtime's schema validation. `about(text)` adds the description the agent reads.
function spec(definition) {
    return { ...definition, about: (text) => ({ ...definition, schema: { ...definition.schema, description: text } }) };
}

export const field = {
    choice: (values, initial) => spec({
        schema: { type: "string", enum: values },
        initial,
        check: (value, name) => {
            if (!values.includes(value)) throw invalid(name, `one of ${values.join(", ")}`);
            return value;
        },
    }),
    text: (initial = "", maxLength = 200) => spec({
        schema: { type: "string", maxLength },
        initial,
        check: (value, name) => {
            if (typeof value !== "string" || value.length > maxLength) throw invalid(name, `a string of at most ${maxLength} characters`);
            return value;
        },
    }),
    flag: (initial = false) => spec({
        schema: { type: "boolean" },
        initial,
        check: (value, name) => {
            if (typeof value !== "boolean") throw invalid(name, "true or false");
            return value;
        },
    }),
    // A set of ids (or of `values` when given); stored sorted and de-duplicated.
    ids: (values = null, max = 500) => spec({
        schema: { type: "array", maxItems: max, items: values ? { type: "string", enum: values } : { type: "string", maxLength: 300 } },
        initial: [...(values ?? [])].sort(),
        check: (value, name) => {
            if (!Array.isArray(value) || value.length > max) throw invalid(name, `an array of at most ${max} ids`);
            for (const id of value) {
                if (typeof id !== "string" || id.length > 300 || (values && !values.includes(id))) throw invalid(name, `unknown id ${JSON.stringify(id)}`);
            }
            return [...new Set(value)].sort();
        },
    }),
    // A non-negative integer, or one of the named `words` ("auto", "all", ...).
    index: (words, initial) => spec({
        schema: { anyOf: [{ type: "integer", minimum: 0 }, { type: "string", enum: words }] },
        initial,
        check: (value, name) => {
            if (!(Number.isSafeInteger(value) && value >= 0) && !words.includes(value)) throw invalid(name, `a zero-based index or one of ${words.join(", ")}`);
            return value;
        },
    }),
    // An integer from `min` to `max`.
    range: (min, max, initial) => spec({
        schema: { type: "integer", minimum: min, maximum: max },
        initial,
        check: (value, name) => {
            if (!Number.isSafeInteger(value) || value < min || value > max) throw invalid(name, `an integer from ${min} to ${max}`);
            return value;
        },
    }),
    // Per-id open/closed overrides of a default; `{ id: boolean }`.
    toggles: (max = 500) => spec({
        schema: { type: "object", maxProperties: max, additionalProperties: { type: "boolean" } },
        initial: {},
        check: (value, name) => {
            if (value === null || typeof value !== "object" || Array.isArray(value)) throw invalid(name, "an object of id: boolean");
            const entries = Object.entries(value);
            if (entries.length > max || entries.some(([id, open]) => id.length > 300 || typeof open !== "boolean")) throw invalid(name, "an object of id: boolean");
            return Object.fromEntries(entries);
        },
    }),
};

export class ViewState {
    #fields;
    #values;
    #scopeKey;
    #listeners = new Set();
    version = 0;

    constructor(fields) {
        this.#fields = fields;
        this.#values = Object.fromEntries(Object.entries(fields).map(([name, spec]) => [name, structuredClone(spec.initial)]));
    }

    get() {
        return structuredClone(this.#values);
    }

    // Applies a partial update; unknown or invalid fields reject the whole patch.
    set(patch) {
        if (patch === null || typeof patch !== "object" || Array.isArray(patch)) throw invalid("view", "a JSON object");
        const next = {};
        for (const [name, value] of Object.entries(patch)) {
            const spec = this.#fields[name];
            if (!spec) throw invalid(name, `not a view field; expected ${Object.keys(this.#fields).join(", ") || "none"}`);
            next[name] = spec.check(value, name);
        }
        Object.assign(this.#values, next);
        return this.get();
    }

    // Saved values from an earlier provider; fields that no longer exist or validate keep their default.
    restore(values) {
        for (const [name, value] of Object.entries(values ?? {})) {
            try {
                this.set({ [name]: value });
            } catch {
                // Keep the default.
            }
        }
    }

    // Resets fields to their initial values, e.g. when the selection they describe goes away.
    reset(...names) {
        for (const name of names) this.#values[name] = structuredClone(this.#fields[name].initial);
    }

    // Ties `names` to what they describe (the attempt a step belongs to, ...): when `key` differs from
    // the last one seen, they reset and pages are told, even when a read noticed the change.
    scope(key, ...names) {
        const moved = this.#scopeKey !== undefined && this.#scopeKey !== key;
        this.#scopeKey = key;
        if (!moved) return;
        this.reset(...names);
        this.changed();
    }

    // Any change a page should re-render for, view or data (a run started, a baseline accepted, ...).
    changed() {
        this.version += 1;
        for (const listener of this.#listeners) listener(this.version);
    }

    subscribe(listener) {
        this.#listeners.add(listener);
        return () => this.#listeners.delete(listener);
    }
}
