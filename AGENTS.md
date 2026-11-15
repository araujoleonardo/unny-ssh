# Orientações para agentes

## Projeto

O `unny-ssh` é um aplicativo desktop com Tauri 2, backend em Rust e
frontend em TypeScript com Vite. Consulte o `README.md` para a preparação
do ambiente.

## Estrutura

- `src/`: código e estilos do frontend.
- `index.html`: entrada HTML do frontend.
- `src-tauri/src/`: código Rust e comandos Tauri.
- `src-tauri/tauri.conf.json`: configuração do aplicativo.
- `src-tauri/capabilities/`: permissões do Tauri.
- `docs/`: documentação do projeto.

## Desenvolvimento

- Use npm para gerenciar as dependências do frontend.
- Execute `npm run tauri dev` para desenvolver o aplicativo completo.
- Execute `npm run dev` para trabalhar apenas com o frontend no navegador;
  recursos dependentes do Tauri precisam do aplicativo desktop.
- Siga os padrões existentes e mantenha as alterações focadas na tarefa.
- Ao alterar comandos Tauri, mantenha os contratos entre Rust e TypeScript
  consistentes.
- Não inclua credenciais, chaves privadas ou outros segredos no código,
  nos logs ou na documentação.
- Evite editar artefatos gerados em `dist/`, `node_modules/`,
  `src-tauri/target/` e `src-tauri/gen/`.
- Documente mudanças relevantes de comportamento, configuração e uso em
  `docs/` ou no `README.md`, conforme o contexto.

## Validação

Execute as verificações pertinentes às alterações:

- Frontend: `npm run build`.
- Rust: `cargo check --manifest-path src-tauri/Cargo.toml`.
- Formatação Rust: `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`.
- Empacotamento, quando necessário: `npm run tauri build`.

Ao concluir, informe o que mudou, quais verificações foram executadas e
eventuais limitações. Para alterações apenas na documentação, revise o
conteúdo e os caminhos; não é necessário gerar o aplicativo.
