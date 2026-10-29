// A page message: errors, warnings, progress, or confirmations, on Primer's Banner.
import { Banner } from "@primer/react";
import type { ReactNode } from "react";
import "./Notice.css";

type Tone = "error" | "warning" | "info" | "ok";
const VARIANTS = { error: "critical", warning: "warning", info: "info", ok: "success" } as const;

interface Props {
  tone: Tone;
  title: ReactNode;
  children?: ReactNode;
  // Shows `children` as preformatted output (e.g. a process's stderr).
  pre?: boolean;
  className?: string;
}

export function Notice({ tone, title, children, pre, className }: Props) {
  const description = children == null ? undefined : pre ? <pre className="notice-pre">{children}</pre> : children;
  return (
    <Banner
      className={`notice banner ${tone}${className ? ` ${className}` : ""}`}
      variant={VARIANTS[tone]}
      layout="compact"
      description={description}
    >
      {/* As a child, since the `title` prop also types as the HTML attribute (string only). */}
      <Banner.Title>{title}</Banner.Title>
    </Banner>
  );
}

export function Notices({ children }: { children: ReactNode }) {
  return <div className="notices" aria-live="polite">{children}</div>;
}
