# Evidências das tasks 1, 2 e 3

Data: 14/11/2026. Equipamento: macOS/Darwin arm64, Apple M4, 16 GiB de RAM.
Esta máquina não representa ainda o equipamento mínimo de 8 GiB do aceite final.

## Implementação

- Removidos `greet`, a página de exemplo e o plugin opener. Adicionados módulos
  de aplicação, features, IPC, comandos, serviços e adaptadores, preservando
  TypeScript/Vite sem framework e npm. Monaco e xterm são carregados sob demanda.
- AppError com códigos seguros, UUID de correlação, estados de operação, IDs
  opacos e geração. Comandos de domínio rejeitam cofre bloqueado; não existe
  registro de sessões reais nesta etapa, portanto IDs de sessão são desconhecidos.
- Channels: no máximo 16 streams, 32 mensagens pendentes por stream, 64 KiB por
  mensagem e uma mensagem em trânsito aguardando ACK. ACK incorreto e geração
  expirada são rejeitados; final libera o slot, cancelamento remove a fila e o
  bloqueio invalida todos os streams. O cliente descarta outra operação, sequência
  repetida/saltada e eventos antigos. Snapshots cumulativos de progresso são
  agregados em até 10 Hz, com estado final sempre entregue. Produtores reais
  serão integrados nas respectivas tasks de operações remotas.
- CSP sem CDN/eval, workers locais e permissões de comandos concedidas apenas à
  janela principal. Dev CSP permite os WebSockets locais do Vite. Não há acesso
  genérico da UI a shell, filesystem ou abertura de links. Instância única e lock
  de arquivo protegem a publicação do cofre.
- Layout Islands Dark: superfícies grafite, separação em ilhas, acento azul claro,
  foco visível e componentes com contraste verificado por pares de tokens. Lateral
  e terminal aceitam mouse/teclado, recolhimento e restauração; persistência local
  validada e limitada a dimensões e flags não sensíveis. Todo contexto remoto é
  explicitamente simulado. UI de erro, carregamento e diálogos usa texto/ARIA.
- Cofre v1: cabeçalho autenticado, salt de 16 bytes, nonce de 24 bytes renovado em
  cada geração, chave de 256 bits e Argon2id v19. Custos aceitos nesta versão são
  exatamente 65536 KiB / 3 / 1; outros valores falham antes da KDF. Limite de arquivo
  de 16 MiB e de senha de 1024 bytes. Buffers controláveis usam `Zeroizing`.
- Temporário criptografado no mesmo filesystem, sync e substituição atômica via
  `atomicwrites` (rename/sync em Unix; MoveFileEx com WRITE_THROUGH no Windows).
  A troca de senha valida a geração nova em memória antes de publicar. O arquivo
  anterior permanece publicado até a substituição. Interrupções antes do commit
  recuperam o anterior; depois do commit recuperam a geração nova. Temporários de
  um processo morto são ignorados e não substituem um cofre inválido.
- Payload reserva perfis, segredos, hosts conhecidos e referências de blobs de
  rascunho por UUID/geração. Nenhum rascunho é produzido ou persistido nesta etapa;
  publicação de múltiplos blobs e bloqueio integrado pertencem à task 10.
- Criar/desbloquear/bloquear/trocar senha/redefinir usam IPC real. KDF e disco rodam
  no pool bloqueante; comandos síncronos respondem BUSY durante essas operações.
  Redefinição exige texto de perda e token de uso único, vinculado à geração e com
  expiração de cinco minutos. Campos de senha são limpos antes do await do IPC.
- Logs excluem payloads, hosts, caminhos, perfis, conteúdo e credenciais. Contêm
  somente operação, resultado e duração; falha de logging não impede recuperação.

## Validações executadas

| Verificação | Resultado neste equipamento |
| --- | --- |
| `npm test` | 7 testes aprovados em 5 arquivos |
| `npm run build` | Aprovado; worker local incluído no bundle |
| `cargo test --manifest-path src-tauri/Cargo.toml` | 13 testes aprovados; 3 helpers ignorados por padrão |
| `cargo check --manifest-path src-tauri/Cargo.toml` | Aprovado |
| `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check` | Aprovado |
| `npm run tauri build -- --debug --no-bundle` | Aprovado |
| `npm run tauri build -- --debug --bundles app` | Bundle macOS gerado |
| `npm audit` | Zero vulnerabilidades após override DOMPurify 3.4.17 |
| `ruby -c tests/fixtures/docker-vm/Vagrantfile` | Sintaxe aprovada; VM não iniciada |

Os testes Rust cobrem senha incorreta, preservação de payload não vazio durante
rotação, nova senha invalidando a antiga, nonce novo, adulteração de cabeçalho e
ciphertext, limites KDF, cofre inválido preservado, lock concorrente, permissões
0600, erros de escrita injetados, falha real de rename por permissões Unix e
encerramento de subprocessos antes/durante/depois da publicação. Também cobrem
contratos Serde, confirmação expirada/uso único e streams lentos/cancelados/finais.
O erro injetado de escrita não equivale a esgotar fisicamente um disco real.

Vitest cobre preferências inválidas, persistência, teclado nos divisores, troca de
contexto em todos os painéis, navegação Docker, recolhimento/restauração, limpeza e
confirmação de senha, isolamento de eventos e contraste dos pares de tokens.

Benchmark executado separadamente:

```sh
cargo test --release --manifest-path src-tauri/Cargo.toml benchmark_kdf -- --ignored --nocapture
```

Uma derivação Argon2id mediu **96,95 ms** com 64 MiB, t=3, p=1 em release.
Criação e troca de senha fazem também validação criptográfica da geração nova,
portanto a duração total é maior do que uma derivação isolada. Debug é mais lento.

## Smoke desktop macOS

Foi usado um diretório temporário, criado pelo helper
`services::vault::native_fixture::seed`, com credencial exclusivamente sintética.
A variável `UNNY_SMOKE_DATA_DIR` só é reconhecida em builds debug; release sempre
usa o diretório de dados do aplicativo. Não foi usado cofre pessoal.

Verificados na WebView `tauri://localhost` do bundle macOS:

- Tela bloqueada ao iniciar e desbloqueio por IPC real.
- Monaco montado com arquivo shell local e xterm com saída sintética.
- Troca Sandbox → Staging atualizando lateral, área central, terminal e status.
- Divisor lateral por teclado de 260 para 280 px, preservado após bloquear/abrir.
- Navegação Docker, recolhimento/restauração do terminal com Cmd+J.
- Bloqueio removendo editor e terminal; novo desbloqueio com a fixture.
- Diálogo de troca de senha acessível e fechamento por Escape.

O smoke visual não inspecionou o console da WKWebView nem comprovou handshake do
worker por instrumentação. O asset do worker foi empacotado; a montagem do editor
foi observada. Após o smoke, comentários do tema receberam contraste maior e o
fechamento do diálogo passou a restaurar foco explicitamente no botão de origem.
A ferramenta Chrome DevTools não encontrou Chrome instalado; não se confunde esse
resultado com falha do aplicativo. Não foi executado E2E web Playwright.

## Pendências de aceite

- Executar a CI nativa: este workspace não contém metadados Git/remoto e não há
  runners Windows/Linux conectados. Workflow criado, sem execução remota presumida.
- Smoke completo nos três sistemas: fontes, scroll, workers/CSP, foco, criação,
  rotação, reset e ACL Windows. A matriz fica pendente mesmo com testes de serviço
  aprovados no macOS. Criação, rotação e reset tiveram teste de serviço, não fluxo
  completo por cliques no desktop.
- Provisionar efetivamente OpenSSH/SFTP e VM Docker. Foram entregues receitas
  descartáveis; nenhum daemon de produção foi usado. Compatibilidade SSH é task 5.
- Testar disco fisicamente cheio e falhas de energia/fsync em cada SO. Erro após
  rename, como falha de sync do diretório, pode ter resultado de commit incerto;
  preservar arquivos e reabrir para determinar qual geração foi publicada.
- Exercitar perda de foco, mouse/arraste e contraste computado completo nas WebViews
  dos três sistemas. A medição por tokens não é auditoria automática de toda a UI.
- Rascunhos/blob set e sessões/operações reais entram nas tasks posteriores;
  não declarar bloqueio integrado entregue pela task 3.
- O bundle Monaco gera aviso de chunk acima de 500 KiB. Ele já está em import
  dinâmico, fora da tela de desbloqueio. Não houve atualização incidental de
  Vite/TypeScript. DOMPurify recebeu override pontual por alertas da dependência
  nova do Monaco; manter o override até uma versão compatível corrigir a resolução.

Referências consultadas: [Argon2 RustCrypto](https://docs.rs/argon2/0.5.3/argon2/),
[XChaCha20-Poly1305 RustCrypto](https://docs.rs/chacha20poly1305/0.10.1/chacha20poly1305/),
[Channels Tauri](https://v2.tauri.app/develop/calling-rust/) e
[repositório apt oficial do Docker](https://docs.docker.com/engine/install/ubuntu/).
