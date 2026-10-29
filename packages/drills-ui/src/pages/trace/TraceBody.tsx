// The page body under the header: why there is nothing to show, or the traced attempt's notices,
// steps, step detail, and telemetry.
import { BeakerIcon, HistoryIcon } from "@primer/octicons-react";
import { Spinner } from "@primer/react";
import { EmptyState } from "../../components/EmptyState";
import { ErrorText } from "../../components/ErrorText";
import { Notice } from "../../components/Notice";
import { StepDetail } from "./StepDetail";
import { StepsSection } from "./StepsSection";
import { TelemetrySection } from "./TelemetrySection";
import { shownStep, type TraceState } from "./types";

interface Props {
  state: TraceState | null;
  loadError: string | null;
  pick: (step: number | "all") => void;
  toggleKind: (kind: string) => void;
}

export function TraceBody({ state, loadError, pick, toggleKind }: Props) {
  if (!state) return loadError ? null : <div className="loading"><Spinner size="medium" srText="Loading" /></div>;
  if (!state.run) {
    return state.pinned
      ? <EmptyState icon={HistoryIcon} title={<>Run <span className="path">{state.pinned}</span> was not found.</>}>Pick another run, or go back to the latest.</EmptyState>
      : <EmptyState icon={HistoryIcon} title="No drill runs yet">Runs appear here from <span className="path">.auditaur/drill-runs</span> once a drill runs.</EmptyState>;
  }
  const selected = state.selected;
  if (!selected) return <EmptyState icon={BeakerIcon} title="This run has no tests." />;
  const step = shownStep(selected, state.ui.step);
  const kinds = state.ui.kinds;
  // The failing step repeats the attempt's error; show it once, with that step.
  const stepError = step == null ? null : selected.steps[step]?.error;
  const attemptError = selected.error && selected.error !== stepError ? selected.error : null;
  return (
    <>
      {selected.missing && <Notice tone="info" title="This attempt has not written its report yet." />}
      {attemptError && <ErrorText className="attempt-error">{attemptError}</ErrorText>}
      <StepsSection selected={selected} kinds={kinds} step={step} pick={pick} toggleKind={toggleKind} />
      <StepDetail selected={selected} runId={state.run.runId} step={step} showAll={() => pick("all")} />
      <TelemetrySection selected={selected} kinds={kinds} step={step} />
    </>
  );
}
