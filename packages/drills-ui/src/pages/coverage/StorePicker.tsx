// Which app telemetry stores count as the app's surface, so commands and events no test touched show
// up. Ticking boxes is local to the page until the apply button saves the choice.
import { DatabaseIcon } from "@primer/octicons-react";
import { Button, Spinner } from "@primer/react";
import { useEffect, useState } from "react";
import { Checklist } from "../../components/Checklist";
import { Notice } from "../../components/Notice";
import { plural } from "../../lib/format";
import { useAppStores } from "./useAppStores";

interface Props {
  dbs: string[];
  busy: boolean;
  apply: (dbs: string[]) => void;
}

const same = (a: ReadonlySet<string>, b: string[]) => a.size === b.length && b.every((db) => a.has(db));

export function StorePicker({ dbs, busy, apply }: Props) {
  const [open, setOpen] = useState(false);
  const [chosen, setChosen] = useState<ReadonlySet<string>>(() => new Set(dbs));
  const stores = useAppStores(open, dbs);
  const inUse = dbs.join("\n");

  // Opening, or the agent changing the stores, starts again from what is in use.
  useEffect(() => {
    setChosen(new Set(inUse ? inUse.split("\n") : []));
  }, [open, inUse]);

  const toggle = (value: string, on: boolean) =>
    setChosen((current) => {
      const next = new Set(current);
      if (on) next.add(value);
      else next.delete(value);
      return next;
    });
  const changed = !same(chosen, dbs);

  return (
    <details id="stores" className="coverage-stores" onToggle={(event) => setOpen(event.currentTarget.open)}>
      <summary>
        <DatabaseIcon size={16} className="coverage-stores-icon" />
        <span className="coverage-stores-title">Compare with app telemetry</span>
        <span className="muted coverage-stores-using">{dbs.length ? `Using ${plural(dbs.length, "store")}` : "Shows only what the run touched"}</span>
      </summary>
      <div className="coverage-stores-body">
        <p className="coverage-stores-lede muted">
          Pick the stores of app sessions whose commands and events count as the app's surface. Anything they recorded that no test exercised is listed as unexercised.
        </p>
        <div id="store-error">{stores.error && <Notice tone="error" title={`Could not list app stores: ${stores.error}`} />}</div>
        {!stores.items ? (
          <div id="store-rows" className="coverage-stores-loading"><Spinner size="small" srText="Loading app stores" /></div>
        ) : stores.items.length ? (
          // Locked while saving, so a tick made mid-save can't be overwritten by the saved choice.
          <fieldset className="coverage-stores-choices" disabled={busy}>
            <legend className="visually-hidden">App telemetry stores</legend>
            <Checklist
              items={stores.items}
              chosen={chosen}
              toggle={toggle}
              noun="store"
              filterId="store-filter"
              filterLabel="Filter app stores"
              placeholder="Filter by app, session or path"
              rowsId="store-rows"
            />
          </fieldset>
        ) : (
          <div id="store-rows" className="muted coverage-stores-none">No Auditaur app sessions found. Run your app with Auditaur enabled, then reopen this list.</div>
        )}
        {changed && (
          <div className="row">
            <Button id="apply-stores" variant="primary" size="small" disabled={busy} onClick={() => apply([...chosen])}>
              {chosen.size ? `Compare with ${plural(chosen.size, "telemetry store")}` : "Stop comparing with app telemetry"}
            </Button>
          </div>
        )}
      </div>
    </details>
  );
}
