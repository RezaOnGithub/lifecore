import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { App } from "./App.tsx";
import { RpcClient } from "./rpc.ts";
import { takeLaunchToken } from "./token.ts";

const token = takeLaunchToken({
  location: window.location,
  history: window.history,
  storage: window.sessionStorage,
});
const root = document.getElementById("root");
if (root === null) {
  throw new Error("index.html has no #root element");
}
createRoot(root).render(
  <StrictMode>
    <App client={new RpcClient(token)} />
  </StrictMode>,
);
