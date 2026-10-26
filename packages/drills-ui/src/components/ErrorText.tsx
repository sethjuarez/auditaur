// A failure message (an assertion, a stack) as preformatted text in the danger role.
import "./ErrorText.css";

export function ErrorText({ children, className }: { children: string; className?: string }) {
  return <pre className={`detail-error${className ? ` ${className}` : ""}`}>{children}</pre>;
}
