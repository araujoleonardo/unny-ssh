# Feature Status

## Overall Progress: 0%

## Current Status: In Progress

## Task Progress

- [ ] 1.0 [Estrutura modular, contratos IPC e ambiente de testes](1.0_task.md): Ready for QA
- [ ] 2.0 [Interface Islands Dark e painéis redimensionáveis](2.0_task.md): Ready for QA
- [ ] 3.0 [Cofre criptografado com senha mestra](3.0_task.md): Ready for QA
- [ ] 4.0 [Cadastro e gerenciamento de conexões](4.0_task.md): Ready for QA
- [ ] 5.0 [SSH, identidade do servidor e múltiplas sessões](5.0_task.md): Ready for QA
- [ ] 6.0 [Terminal inferior com abas](6.0_task.md): Ready for QA
- [ ] 7.0 [Explorador remoto por SFTP](7.0_task.md): Ready for QA
- [ ] 8.0 [Monaco e gravação remota](8.0_task.md): Ready for QA
- [ ] 9.0 [Download, ZIP e upload](9.0_task.md): Ready for QA
- [ ] 10.0 [Bloqueio integrado e recuperação de rascunhos](10.0_task.md): Ready for QA
- [ ] 11.0 [Containers, imagens e volumes Docker](11.0_task.md): Ready for QA
- [ ] 12.0 [Gerenciamento de projetos Docker Compose](12.0_task.md): Ready for QA
- [ ] 13.0 [Aceite e distribuição multiplataforma](13.0_task.md): In Progress

Concluídas por aceite: 0 de 13 tarefas principais. Código das tasks 1–12 entregue,
com testes locais e smoke macOS; aceite multiplataforma ainda pendente. O percentual
permanece em 0% porque tasks principais só contam após seus critérios e testes
pertinentes serem aprovados. [Evidências 1–3](validation-1-3.md), [4–6](validation-4-6.md), [7–9](validation-7-9.md) e [10–12](validation-10-12.md).

## Blockers & Risks

- Conexões JSON: importação/exportação `ssh-client-backup` v1 implementada com
  exclusão de credenciais, pastas/metadados preservados, duplicidades ignoradas e
  importação atômica. Smoke de diálogos/arquivo real aprovado no macOS; clientes
  Windows/Linux pendentes. As pastas ainda não agrupam a lista visual. Detalhes
  e evidências em [connections-json.md](../connections-json.md).

- Pré-instalação macOS: metadados, mínimo declarado 13.0 e arraste da janela
  ajustados; configuração de assinatura ad hoc local separada. Ambiente ARM64
  sem identidades Developer ID válidas. DMG, assinatura/notarização pública,
  instalação final, execução no macOS 13 e Intel/universal pendentes. Evidências
  e comandos em [macos-release.md](../macos-release.md).

- Tasks 10–12: pendentes clientes Windows/Linux, eventos físicos de suspensão,
  falhas físicas de disco/rede e matriz completa das ações na UI. Em Windows/Linux,
  suspensão é detectada por visibilidade/intervalo na retomada. Docker usa socket
  Linux padrão; cancelamento não promete rollback de efeitos remotos. Detalhes em
  [validation-10-12.md](validation-10-12.md).

- Arquivos/transferências: QA Windows/Linux, drop pelo SO, fallback sem rename POSIX,
  disco cheio, interrupção física e ZIP multigigabyte pendentes. APFS recusou nomes
  não UTF-8; prova por protocolo aprovada, filesystem Linux pendente. Limitações
  de concorrência SFTP e bloqueio com dirty em [validation-7-9.md](validation-7-9.md).

- Tasks 1–12 implementadas e disponíveis para QA; não marcar como Completed antes
  da matriz nativa. Workflow macOS/Windows/Linux criado; ainda não executado remotamente.
- CI remota ainda não executada nesta etapa.
- Docker/Compose foram executados em VM Ubuntu 24.04 ARM64 isolada, criada somente
  para os testes. Cliente macOS e servidor Linux comprovados; não confundir com
  aceite da WebView do cliente Linux. Receita Vagrant anterior não executada.
- ACL Windows, handshake/CSP dos workers e fluxos completos do cofre por cliques
  precisam de validação nativa. Disco cheio real e falhas de sync também pendentes.
- Compatibilidade: russh, formatos Ed25519/RSA criptografados e Agent passaram no
  macOS. Faltam Windows/Linux, Agent named pipe, diálogos e clipboard na matriz.
  Workers/CSP e persistência atômica continuam exigindo aceite nos três sistemas.
- Rascunhos: reinício, rotação/falha de senha, IO injetado e permissão negada real
  aprovados. ENOSPC físico, interrupção de energia, limites máximos e ACL Windows
  ainda pendentes. `lock_failed` retém chave/buffers e exige autenticação.
- SFTP: gravação tem janela residual de concorrência sem compare-and-swap; comunicar
  e testar as limitações (8.0).
- Release: acesso a runners/máquinas dos três sistemas, ambiente descartável
  Docker e certificados serão necessários; disponibilidade ainda não verificada.
- Design: não existe design.md. PRD e spec orientam o layout; decisões visuais
  foram registradas em validation-1-3.md para a task 2.0.

## Open Questions

- Matriz mínima: confirmar macOS 13+, Windows 10 22H2+ e Ubuntu 22.04+ propostos
  na spec antes da distribuição (13.0).
- Bibliotecas: fixar versões depois das provas de compatibilidade (1.0–5.0).
- Aceite: definir equipamento exato de referência e recrutar cinco usuários (13.0).
- Distribuição macOS: fluxo local `.app`/DMG preparado; definir canal público,
  disponibilizar Developer ID/notarização e decidir ARM64 ou universal (13.0).
- Premissas do PRD: manter uso individual e servidores Linux até decisão diferente.

## Changelog

- 2026-11-14: Adicionada importação/exportação JSON sem senhas, passphrases ou
  chaves privadas, conforme arquivo fornecido. Schema validado no backend,
  importação aditiva atômica, metadados persistidos no cofre e exportação por
  whitelist. 39 testes frontend, 48 Rust, check/fmt/build e smoke nativo macOS
  aprovados. Release local ARM64 recompilada e assinatura ad hoc validada.
  Nenhum checkbox de aceite multiplataforma foi concluído.

- 2026-11-14: Iniciada parcialmente a task 13 na revisão pré-instalador macOS.
  37 testes frontend, 41 Rust e integrações nativas OpenSSH/SFTP aprovados;
  build/check/fmt e auditoria npm de produção aprovados. Configurados metadados,
  mínimo macOS 13.0, arraste da janela e build local ad hoc; certificados Apple
  ausentes. Aceite multiplataforma permanece pendente; nenhum checkbox principal
  foi concluído.

- 2026-11-14: Implementadas tasks 10–12: coordenador de bloqueio, inatividade,
  blobs de rascunhos criptografados/recuperação/rotação e lock_failed autenticado;
  executor Docker estruturado, inventário/ações/inspect/logs/exec, projetos Compose
  criptografados e confirmação adicional para volumes. 33 testes frontend, 40 Rust,
  quatro integrações Docker/bloqueio na VM e regressões OpenSSH/SFTP aprovados.
  Smoke macOS comprovou falha real de permissão/recuperação e fluxos Docker/Compose.
  Ready for QA, com evidências em validation-10-12.md. Aceite permanece 0%.

- 2026-11-14: Implementadas tasks 7–9: árvore SFTP lazy/virtualizada, paths/handles
  por bytes, CRUD confirmado, Monaco por sessão, revisão/hash e save com temporário,
  conflitos, guards de saída, seleção nativa, fila de transferências, ZIP64,
  colisões/cancelamento/resíduos. 22 testes frontend, testes Rust e integração SFTP
  real aprovados; build/bundle macOS e smoke de edição/download/upload/bloqueio/saída.
  Ready for QA; detalhes e pendências em validation-7-9.md. Aceite continua 0%.

- 2026-11-14: Implementadas tasks 4–6: CRUD criptografado de perfis, importação
  autorizada de chaves, credenciais salvas/transitórias, identidade de host antes
  da autenticação, reviews com expiração/concorrência, sessões SSH e terminais PTY.
  Fluxos por senha, chave criptografada e Agent; isolamento, ACK, limites de buffers,
  Ctrl+C, resize e cancelamento. 17 testes Vitest, 24 Rust e uma integração nativa
  OpenSSH/Agent/SFTP/PTy aprovados no macOS; bundle gerado. Smoke UI confirmou
  terminal real, duas abas, edição/duplicação/busca e bloqueio. Ready for QA;
  pendências da matriz em validation-4-6.md. Percentual de aceite permanece 0%.

- 2026-11-14: Implementadas tasks 1–3: módulos/IPC/streams limitados, cofre
  Argon2id/XChaCha20-Poly1305, escrita atômica, lock, logs seguros, CI e fixtures;
  UI Islands Dark com Monaco/xterm locais e layout persistente. 7 testes Vitest e
  13 testes Rust aprovados, build/check/fmt e bundle macOS. Smoke nativo macOS de
  desbloqueio, troca de contexto, teclado, Docker e bloqueio. KDF release: 96,95 ms
  em Apple M4/16 GiB. Sem aprovação presumida de Windows/Linux. Estado Ready for QA.

- 2026-11-13: Sequência de 13 tarefas aprovada pelo usuário; criados tasks.md,
  arquivos individuais e estado inicial. Nenhum código implementado nesta etapa.

