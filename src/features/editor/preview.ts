import * as monaco from "monaco-editor/editor/editor.api.js";
import EditorWorker from "monaco-editor/editor/editor.worker.js?worker";
import "monaco-editor/languages/definitions/shell/register.js";
self.MonacoEnvironment = { getWorker: () => new EditorWorker() };
monaco.editor.defineTheme("islands", {
  base: "vs-dark",
  inherit: true,
  rules: [
    { token: "comment", foreground: "A6C69B" },
    { token: "string", foreground: "E5B695" },
    { token: "keyword", foreground: "B6C5FF" },
  ],
  colors: {
    "editor.background": "#181a1c",
    "editorLineNumber.foreground": "#686b70",
    "editor.lineHighlightBackground": "#242628",
    "editor.foreground": "#bcbec4",
  },
});
export function mountEditor(element: HTMLElement, server: string) {
  const model = monaco.editor.createModel(
    `# Ambiente de demonstração: ${server}\n# Nenhum comando será executado.\n\n# Este arquivo usa dados de exemplo.\n# Nenhum servidor está conectado.\n\nprintf 'Bem-vindo ao seu espaço de trabalho\\n'\n`,
    "shell",
  );
  const editor = monaco.editor.create(element, {
    model,
    theme: "islands",
    readOnly: true,
    automaticLayout: true,
    minimap: { enabled: false },
    fontFamily: "Menlo, Consolas, monospace",
    fontSize: 13,
    padding: { top: 24 },
    scrollBeyondLastLine: false,
    lineNumbersMinChars: 4,
  });
  return () => {
    editor.dispose();
    model.dispose();
  };
}
