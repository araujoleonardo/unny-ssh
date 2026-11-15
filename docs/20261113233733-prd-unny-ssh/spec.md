# Technical Specification

## Executive Summary

Implementar o [PRD](prd.md) sobre Tauri 2, Rust e TypeScript/Vite existentes.
O repositório contém somente o exemplo `greet`, sem persistência, testes ou
serviços remotos. Não existe `design.md`. Esta especificação define a arquitetura;
tokens visuais e mockups ficam para o design.

O frontend compõe painéis, Monaco e xterm.js. Rust controla cofre, SSH/SFTP,
operações Docker e arquivos locais. Não haverá servidor HTTP local nem serviço
instalado no servidor remoto. Bibliotecas propostas serão fixadas nos lockfiles
após prova de compatibilidade nos três sistemas.

## System Architecture

### Component Overview

- `src/app/`: composição, estado por sessão, navegação, bloqueio e layout Islands
  Dark. Manter TypeScript modular, sem introduzir framework nesta etapa.
- `src/features/{vault,connections,files,editor,terminal,transfers,docker}/`:
  componentes, controladores e estado de apresentação.
- `src/ipc/`: cliente tipado para comandos e streams, sem acesso direto a segredos.
- `src-tauri/src/commands/`: validação de DTOs e adaptação IPC.
- `services/{vault,sessions,files,transfers,docker}/`: regras e ciclo de vida.
- `adapters/`: russh, russh-sftp, armazenamento local e relógio substituíveis em testes.

`AppState` mantém gerenciadores com acesso sincronizado. Uma tarefa assíncrona
possui cada conexão SSH e recebe mensagens; canais separados atendem SFTP,
PTY e execução. Não manter mutex global durante I/O. Trabalho de KDF e ZIP
executa fora do executor assíncrono; filas limitadas aplicam contrapressão.

Fluxo: intenção da UI → comando validado → serviço autorizado pelo estado do
cofre → adaptador → resposta ou stream → atualização somente da sessão de origem.
Todo recurso possui ID opaco, geração de sessão e cancelamento. Bloqueio invalida
as gerações; mensagens atrasadas não repovoam a interface.

## Implementation Design

### Key Interfaces

Contratos ilustrativos, convertidos em DTOs Serde e tipos TypeScript equivalentes:

```text
Vault: create/unlock(password), change_password(old,new), lock(), reset(confirm)
Profiles: list(), put(profile, secretInput?), duplicate(id), remove(id,confirm)
Sessions: connect(profileId, transientAuth?) -> sessionId
          confirm_host(challengeId, decision), disconnect(sessionId)
Files: list(sessionId,path), read(sessionId,path) -> document
       save(sessionId,path,expectedRevision,content,overwriteToken?)
       mutate(sessionId,action,path,target?,confirmation?)
Transfers: start(sessionId,selectionHandle,destination,options) -> operationId
           cancel(operationId)
Terminal: open(sessionId,stream) -> terminalId
          write(terminalId,bytes), resize(terminalId,cols,rows), close(terminalId)
Docker: inventory(sessionId,kind), inspect(sessionId,kind,id)
        act(sessionId,action,target,confirmation?), logs/exec(...,stream)
Compose: register(sessionId,path), act(sessionId,projectId,action,options)
```

Toda operação valida cofre, sessão, propriedade dos IDs e limites. Confirmações
destrutivas usam token curto vinculado à ação, recurso e geração, sem ser uma
simples flag genérica. Terminal aceita entrada arbitrária apenas no canal PTY;
Docker e arquivos usam comandos estruturados, sem aceitar scripts da UI.

### Data Models

- `Profile`: UUID, nome, host, porta, usuário, diretório inicial, modo de
  autenticação, referência opcional de segredo e projetos Compose.
- `Secret`: variante senha, chave importada e passphrase; nunca retornada na
  listagem. Credenciais transitórias não são persistidas.
- `KnownHost`: host/porta, algoritmo e chave pública confirmada.
- `Session`: ID, geração, snapshot do perfil, estado e capacidades detectadas.
- `RemoteEntry`: nome exibível, caminho opaco, tipo, tamanho, permissões e mtime.
- `Document`: ID, sessão, caminho, texto, encoding, revisão e estado dirty.
  `Revision` contém tamanho, mtime e SHA-256 do conteúdo original.
- `Operation`: ID, sessão, tipo, estado, bytes processados, total opcional e erros.
- `AppError`: código estável, mensagem segura, possibilidade de repetição e ID de
  correlação. Códigos incluem `VAULT_LOCKED`, `HOST_KEY_CHANGED`, `CONFLICT`,
  `PERMISSION_DENIED`, `UNSUPPORTED` e `INTERRUPTED`.

Persistência: `appLocalDataDir/vault/`, diretório 0700 e arquivos 0600 em Unix;
ACL restrita ao usuário no Windows. Envelope versionado com salt, parâmetros KDF,
nonce e ciphertext autenticado. Proposta: Argon2id (64 MiB, três iterações,
paralelismo 1; medir antes da entrega) deriva chave de 256 bits; XChaCha20-Poly1305
protege payload e cabeçalho associado. Salt aleatório de 16 bytes e nonce novo
de 24 bytes por gravação, gerados pelo sistema. Usar crates RustCrypto
[Argon2](https://docs.rs/argon2/latest/argon2/) e
[ChaCha20Poly1305](https://docs.rs/chacha20poly1305/latest/chacha20poly1305/).

Payload inclui perfis, segredos, hosts conhecidos e índice de rascunhos.
Rascunhos são blobs criptografados individuais com nomes opacos. Senha e chaves
ficam no backend, com `zeroize` para buffers controláveis; campos de entrada são
limpos após IPC. Não prometer eliminação de todas as cópias da memória da WebView.
Preferências externas contêm somente layout e opções não sensíveis.

Escritas usam temporário criptografado no mesmo diretório, flush/sync e substituição
atômica por adaptador de plataforma; instância única e lock impedem escritores
concorrentes. Troca da senha recriptografa um conjunto completo em geração nova,
publicada somente após validação; manter a anterior até concluir. Cabeçalhos
inválidos ou custos KDF excessivos são rejeitados antes de alocar memória.

### API Endpoints

Não existem endpoints HTTP. Expor comandos Tauri agrupados conforme interfaces
anteriores e DTOs explícitos; remover `greet`. Usar
[Channels Tauri](https://v2.tauri.app/develop/calling-rust/)
para terminal, logs e progresso, com sequência, geração e mensagem final.
Streaming limitado em memória, ACK da UI e cancelamento evitam filas ilimitadas;
progresso é agregado em até dez atualizações por segundo.

## Integration Points

**SSH/SFTP:** usar `russh` com `russh-sftp`; validar chave do host antes de enviar
credenciais. Hosts novos geram desafio de confirmação com expiração; mudança de
chave bloqueia até revisão explícita. Agent usa socket Unix ou named pipe OpenSSH
no Windows, mecanismos presentes no
[cliente russh](https://docs.rs/russh/latest/src/russh/keys/agent/client.rs.html).
Não habilitar agent forwarding. Alvo inicial: chaves OpenSSH Ed25519 e RSA com
assinaturas SHA-2, incluindo chaves criptografadas; comprovar formatos por fixtures.
VPN somente altera a alcançabilidade. Timeout inicial de 15 s; sem repetição de
comandos mutáveis após queda. SFTP indisponível desabilita arquivos, preservando PTY.

**Arquivos:** árvore sob demanda, listagens virtualizadas; edição limitada a
5 MiB, texto UTF-8 com preservação de BOM e finais de linha. Outros encodings e
binários oferecem download. Antes de salvar, reler e comparar revisão; gravar
temporário irmão e usar rename atômico quando suportado, preservando permissões.
Sem substituição atômica, informar limitação e exigir aceite antes do fallback.
SFTP não oferece compare-and-swap universal: existe janela entre comparação e
rename, documentada como risco, sem promessa de proteção contra escrita concorrente.

Nota de implementação (14/11/2026): `russh-sftp` 3.0.1 expõe paths e handles
como String. O adaptador SFTP v3 em `src-tauri/src/adapters/sftp.rs` reutiliza
seus atributos/serialização sobre o canal russh e preserva paths/handles em bytes,
com limites de pacote e validação próprios. A prova de interoperabilidade e as
limitações estão em [validation-7-9.md](validation-7-9.md).

**Transferências:** seletor nativo entrega handles de arquivos autorizados;
backend realiza streaming de 64 KiB, até duas transferências por sessão.
ZIP é montado localmente via crate `zip`, sem exigir utilitário remoto. Não seguir
symlinks no empacotamento; informar os ignorados. Normalizar entradas, rejeitar
travessia e nomes duplicados; suportar ZIP64. Resultados incompletos ficam `.part`
e nunca recebem nome final. Upload usa temporário remoto e decisão de colisão;
cancelar tenta limpar temporários e reporta resíduos. Não há retomada automática.

**Monaco/terminal:** empacotar workers pelo
[mecanismo ESM/Vite do Monaco](https://github.com/microsoft/monaco-editor/blob/main/docs/integrate-esm.md),
sem CDN. Modelos identificados por sessão/caminho são descartados no fechamento.
`@xterm/xterm` e addon-fit renderizam PTY e resize; scrollback limitado a 5.000
linhas. Desabilitar clipboard remoto e abertura automática de links, tratando
saída como não confiável conforme o [guia xterm](https://xtermjs.org/docs/guides/security/).

**Docker:** executar CLI remota via SSH, JSON para inventário e inspeção, exit
status para resultado; Compose v2 por diretório registrado. Fazer quoting POSIX
centralizado de cada argumento e rejeitar NUL; não concatenar entrada sem escape.
Resolver IDs e comandos por allowlist, contexto Docker local ao servidor e sem
sudo automático. Logs/exec usam canais separados. `down` não inclui `--volumes`
sem confirmação específica, conforme a
[semântica do Compose](https://docs.docker.com/reference/cli/docker/compose/down/).

Nota de implementação (14/11/2026, tasks 10–12): comandos Docker apontam
explicitamente para o socket Linux local padrão `/var/run/docker.sock`, removendo
variáveis de contexto/daemon/TLS e overrides Compose herdados. Não há configuração
de socket rootless alternativo. Projetos são registros no payload criptografado,
associados ao UUID e snapshot de host/porta/usuário do perfil. Comparações de revisão
antes de mutações são verificações observáveis, sem CAS transacional.

## Testing Approach

### Unit Tests

Rust: corrupção, senha incorreta, nonce, limites KDF, migrações, rollback da troca
de senha, quoting, caminhos ZIP, revisões e cancelamento. TypeScript/Vitest:
isolamento de sessões, eventos atrasados, abas dirty, colisões e estados de bloqueio.
Relógio, RNG e adaptadores são injetáveis; produção usa RNG do sistema.

### Integration Tests

OpenSSH/SFTP descartável com usuários e permissões distintos; autenticações,
host alterado, falha de rename, conflitos, symlinks e queda durante transferências.
Docker/Compose em VM descartável, sem usar daemon de produção. Validar containers,
volumes, ausência de ferramentas e permissões. Exercitar Agent nativo em cada SO.

### E2E Tests

Vitest cobre controladores; Playwright cobre frontend com IPC simulado, sem
confundir isso com teste desktop. Fluxos nativos serão validados em macOS,
Windows e Linux: cofre, conexão, edição, ZIP/upload, terminal, Docker, bloqueio e
restauração. Automatizar com driver Tauri onde disponível; checklist manual cobre
lacunas. Medir teclado, contraste, cinco usuários e meta de 200 ms em máquina
documentada com 8 GB RAM/SSD, além de latência de rede simulada.

## Development Sequencing

### Build Order

1. Contratos, erros, adaptadores, shell visual e ambiente de testes.
2. Cofre e perfis, incluindo falhas de persistência e bloqueio.
3. SSH, identidade, autenticações e sessões; validar compatibilidade por SO.
4. Terminal e explorador, depois Monaco e gravação com conflitos.
5. Transferências e ZIP; integrar rascunhos e encerramento seguro.
6. Inventário Docker, ações e Compose, com confirmações.
7. Aceite integrado, acessibilidade, desempenho e empacotamento multiplataforma.

### Technical Dependencies

Adicionar Tokio, russh, russh-sftp, RustCrypto, zeroize, zip, tracing, UUID e
suporte a lock/substituição atômica; frontend Monaco, xterm e Vitest.
Seletores e instância única usam plugins Tauri oficiais. Fixar versões após smoke
tests; não atualizar Vite/TypeScript incidentalmente. Matriz inicial proposta:
macOS 13+, Windows 10 22H2+ e Ubuntu 22.04+, sujeita a confirmação antes da
distribuição. CI nativa e certificados de assinatura são dependências de release.

## Monitoring and Observability

Logs locais estruturados por código, operação, duração e resultado, com rotação
limitada a 10 MiB. Excluir credenciais, nomes de perfis, hosts, caminhos, comandos,
stdout/stderr e conteúdo remoto; inspeções detalhadas aparecem apenas na UI.
Medir latência IPC, bytes, filas e falhas em diagnósticos locais opt-in. Sem
telemetria externa. Registrar cancelamento separado de falha; falha de auditoria
local não deve expor payloads nem bloquear operações de recuperação.

## Technical Considerations

### Key Decisions

Manter TypeScript modular evita migração prematura; serviços independentes deixam
uma futura troca de UI possível. Russh evita depender de OpenSSH instalado e
subprocessos locais. SFTP evita comandos shell para arquivos. CLI Docker via SSH
evita exposição TCP e implementações HTTP/hijack próprias. Cofre em arquivos
criptografados é suficiente para uso individual, sem introduzir banco de dados.

Bloqueio: rejeitar novas operações, ocultar UI, capturar rascunhos, persistir,
cancelar streams e encerrar sessões; só então eliminar chaves e modelos. Em erro
de disco, manter conteúdo apenas em memória, tela protegida e estado
`lock_failed`; permitir autenticar novamente para tentar salvar ou descartar
explicitamente. Não declarar cofre completamente bloqueado enquanto houver
segredos retidos. Inatividade conta interação local, não saída de terminal.

Implementação 10.0: UI é ocultada antes de capturar buffers e bloquear o gate.
Canceladores de consultas, logs, PTY e transferências são acionados antes de
fechar SSH, com até 3 s para sinalizar/fechar canais. Os blobs são publicados em
seguida; chave e modelos só são eliminados após sucesso. A antecipação do
cancelamento impede novas saídas/ações enquanto se protege o conteúdo. Falha
retém buffers/chave no backend, com `lock_failed` e reautenticação para recuperação
ou descarte explícito. Fechar normalmente não contorna esse estado. Blobs usam
nonce/AAD próprios e índice publicado atomicamente; a rotação recriptografa o
conjunto referenciado antes de trocar a geração. A proteção de suspensão usa
notificador NSWorkspace no macOS e visibilidade/intervalo de retomada como fallback.
As provas e as pendências estão em [validation-10-12.md](validation-10-12.md).


### Known Risks

SFTP tem corrida residual de gravação; testar e expor limites. Validar builds,
Agent, formatos de chave, CSP/workers e substituição atômica cedo. Cofre não
protege contra SO comprometido, conteúdo já exportado ou rollback completo de
arquivos. Sem recuperação de senha; não sobrescrever cofre ilegível. Arquivos com
nomes não UTF-8 precisam de handles opacos e exibição escapada.

### Standard Skill Alignment

Seguir `AGENTS.md`: npm, contratos Rust/TypeScript consistentes e nenhuma edição
manual de artefatos gerados. Aplicar `create-spec`; tarefas posteriores seguem
`create-tasks`, e implementação visual segue `frontend-design`.
Restringir comandos à janela principal e ativar
[CSP](https://v2.tauri.app/security/csp/) e
[capabilities](https://v2.tauri.app/security/capabilities/)
mínimas, sem acesso genérico do frontend ao shell/filesystem ou conteúdo remoto
executável. Conferir estado do cofre também dentro de cada serviço.

### Relevant and Dependent Files

Modificar `src/main.ts`, `src/styles.css`, `index.html`, `vite.config.ts`,
`package.json`, `src-tauri/src/lib.rs`, `src-tauri/Cargo.toml`,
`src-tauri/tauri.conf.json` e `src-tauri/capabilities/default.json`.
Criar os módulos descritos acima, `tests/`, fixtures e pipeline nativo.
Atualizar lockfiles pelas ferramentas, `README.md` e `docs/README.md`.
