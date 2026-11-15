import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
export function mountTerminal(element: HTMLElement, server: string) {
  const terminal = new Terminal({
    scrollback: 5000,
    disableStdin: true,
    fontSize: 12,
    fontFamily: "Menlo, Consolas, monospace",
    theme: { background: "#1e2028", foreground: "#dce1ee", green: "#a1d4b3" },
  });
  const fit = new FitAddon();
  terminal.loadAddon(fit);
  terminal.open(element);
  // No link/clipboard handlers or remote escape-sequence extensions are registered.
  terminal.writeln(`\x1b[32m●\x1b[0m ${server} · terminal de demonstração`);
  terminal.writeln(
    "\r\nNenhuma sessão SSH ativa. Este terminal exibe apenas uma prévia.",
  );
  const observer = new ResizeObserver(() => {
    if (element.clientWidth && element.clientHeight) fit.fit();
  });
  observer.observe(element);
  return () => {
    observer.disconnect();
    terminal.dispose();
  };
}
