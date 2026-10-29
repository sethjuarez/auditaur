// The page's toast, pinned to the bottom of the panel.
import { useEffect, useState } from "react";
import { subscribeToast } from "../lib/toast";
import "./Toast.css";

export function Toast() {
  const [text, setText] = useState<string | null>(null);
  useEffect(() => subscribeToast(setText), []);
  return (
    <div id="open-in-toast" className="toast banner error" role="alert" hidden={!text}>
      {text}
    </div>
  );
}
