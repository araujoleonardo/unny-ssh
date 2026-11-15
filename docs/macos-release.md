# Preparação do instalador macOS

Revisão de 14/11/2026, versão 0.1.0. DMG local ARM64 gerado e verificado;
distribuição notarizada e teste de instalação continuam pendentes.

## Ajustes aplicados

- Removidos os metadados de template `A Tauri App` e autor `you`.
- Categoria `DeveloperTool`, descrição do produto e Hardened Runtime explícitos.
- Mínimo declarado de macOS 13.0, conforme a matriz proposta na spec. Isso não
  comprova execução no macOS 13: a máquina disponível usa macOS 27.2.1 ARM64.
- Arraste da barra personalizada autorizado somente para a janela `main`.
  A área central da barra também recebeu `data-tauri-drag-region`.
- Configuração separada `src-tauri/tauri.macos-local.conf.json` para assinar o
  bundle inteiro de testes com identidade ad hoc (`-`). A configuração principal
  preserva a seleção da identidade de distribuição, sem fixar assinatura ad hoc.

## Build local revisável

Em um Mac com os pré-requisitos instalados:

```sh
npm ci
npm test
cargo test --manifest-path src-tauri/Cargo.toml --lib
npm run build:macos:app:local
```

Esse comando gera um `.app` otimizado de produção, sem `--debug`, com assinatura
ad hoc completa. O caminho padrão nesta máquina é:

```text
src-tauri/target/release/bundle/macos/unny-ssh.app
```

Para gerar o DMG local posteriormente:

```sh
npm run package:macos:local
```

Se o executável release já estiver compilado, gere somente o instalador:

```sh
npm run bundle:macos:local
```

Os dois comandos de DMG definem `CI=true`. Nesse modo, o Tauri pula o AppleScript
que personaliza a janela do Finder, evitando o erro de automação `-1743` sem
alterar permissões do macOS. O DMG mantém o aplicativo e o atalho para
Applications, com disposição padrão dos ícones. Não defina
`TAURI_BUNDLER_DMG_IGNORE_CI=true`, pois isso reativa essa etapa.
Esse comportamento está no
[empacotador oficial do Tauri](https://github.com/tauri-apps/tauri/blob/dev/crates/tauri-bundler/src/bundle/macos/dmg/mod.rs).

Os comandos locais usam o target padrão do Mac que compila. Aqui ele é
`aarch64-apple-darwin` (Apple Silicon), sem suporte Intel incluído. Para uma
variante universal, instale os dois targets Rust e valide em ambas as arquiteturas:

```sh
rustup target add aarch64-apple-darwin x86_64-apple-darwin
npm run package:macos:local -- --target universal-apple-darwin
```

O parâmetro de assinatura local não produz identidade Developer ID nem ticket de
notarização. Uma assinatura válida em `codesign --verify` verifica a integridade
local; não equivale à aprovação do Gatekeeper para distribuição pública.

## Distribuição pública

`security find-identity -v -p codesigning` encontrou **zero identidades válidas**
no ambiente revisado. Não foi realizada submissão à Apple. O comando de
notarização está disponível nas Command Line Tools (`xcrun --find notarytool`).

Para publicar, disponibilize a identidade **Developer ID Application** e as
credenciais de notarização no ambiente de build, nunca no repositório. Use a
configuração principal, sem `tauri.macos-local.conf.json`:

```sh
npm run tauri build -- --bundles app,dmg
```

Tauri aceita `APPLE_SIGNING_IDENTITY` e as credenciais Apple documentadas em sua
[orientação oficial de assinatura](https://v2.tauri.app/distribute/sign/macos/).
A [documentação Apple](https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution)
descreve os requisitos de notarização. Depois de assinar/notarizar, confira:

```sh
codesign --verify --deep --strict --verbose=2 src-tauri/target/release/bundle/macos/unny-ssh.app
spctl --assess --type execute --verbose=2 src-tauri/target/release/bundle/macos/unny-ssh.app
xcrun stapler validate src-tauri/target/release/bundle/macos/unny-ssh.app
```

Monte o DMG final, copie o app para Applications em uma máquina de teste e valide
abertura, persistência/reabertura do cofre e os fluxos SSH/editor/transferências/
Docker. O teste deve incluir um download com quarentena, sem desativar Gatekeeper.
Ainda faltam o teste de instalação e o aceite de uma release notarizada.

## Artefato local inspecionado

O `.app` release local tem aproximadamente 9,30 MiB. A inspeção confirmou:

- Mach-O ARM64, versão 0.1.0 e identificador `com.unny.ssh`.
- `LSMinimumSystemVersion` e `LC_BUILD_VERSION/minos`: 13.0.
- Ícone ICNS referenciado pelo bundle presente; `Info.plist` válido.
- `codesign --verify --deep --strict`: aprovado, com recursos selados e flags
  `adhoc,runtime`. Sem `TeamIdentifier`, conforme esperado para identidade `-`.
- `spctl --assess`: **rejected**, conforme esperado para a variante sem Developer
  ID e sem notarização. Não foi desativado ou contornado o Gatekeeper.
- Smoke de abertura da release local no macOS: tela do cofre, logo, estilos,
  input, botão e foco renderizados na WebView nativa. Nenhuma senha foi inserida.
  O fluxo completo após desbloqueio continua pendente nesse artefato release.
- Marcadores de diretório/senha da fixture de smoke ausentes no executável
  release. Isso é uma verificação pontual, não uma auditoria exaustiva de segredos.

## Evidências desta revisão

- 37 testes frontend e 41 testes Rust aprovados; 11 Rust dependentes de fixtures
  ignorados na suíte comum.
- Integrações nativas OpenSSH/Agent/PTY e SFTP/editor/transferências executadas
  separadamente e aprovadas com `tests/fixtures/ssh/run-native.sh` e `--files`.
- `npm run build`, `cargo check` e `cargo fmt -- --check` aprovados.
- `npm audit --omit=dev`: zero vulnerabilidades reportadas. Não equivale a uma
  auditoria de segurança do produto ou das dependências Rust.
- Versões npm/Cargo/Tauri consistentes (0.1.0); ícones configurados existentes.
- CSP de produção mantém IPC/local, sem o WebSocket do servidor de desenvolvimento;
  as capabilities limitam os comandos à janela principal.
- A variável de fixture `UNNY_SMOKE_DATA_DIR` permanece restrita a builds debug.
- Aviso Vite sobre o tamanho do chunk Monaco: não impede o empacotamento, mas
  permanece como oportunidade de otimização.

A task 13 está **In Progress**. Permanecem pendentes aceite completo dos WebViews,
macOS mínimo declarado, Intel/universal, acessibilidade com usuários e métricas de
resposta. Integrações de serviço e revisão no Firefox não substituem esse aceite.

## Atualização: conexões JSON

O `.app` local foi recompilado após a implementação de
[importação e exportação JSON](connections-json.md), com aproximadamente 9,43 MiB.
39 testes frontend e 48 Rust aprovados; smoke nativo macOS de importação/exportação
em cofre descartável e integridade `codesign --verify --deep --strict` aprovados.
A configuração ad hoc, arquitetura ARM64, mínimo declarado e pendências de
notarização/DMG/aceite permanecem os mesmos. O DMG não foi gerado nesta atualização.

## Atualização: DMG sem automação do Finder

Gerado `src-tauri/target/release/bundle/dmg/unny-ssh_0.1.0_aarch64.dmg`
(5,37 MiB) com `CI=true`, usando o executável release existente.
`hdiutil verify` aprovou a integridade. A imagem foi montada somente para leitura:
aplicativo e atalho Applications presentes; `codesign --verify --deep --strict`
aprovado no aplicativo dentro do DMG. A imagem foi desmontada após a validação.
Não houve alteração de permissões do sistema, instalação ou publicação no GitHub.
Continua sendo uma variante com assinatura ad hoc, sem notarização.
