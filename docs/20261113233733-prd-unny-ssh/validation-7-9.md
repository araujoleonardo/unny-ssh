# Evidências das tasks 7–9

Data: 14/11/2026. Ambiente local: macOS, Apple Silicon, Node 24, Rust 1.99.
Estado: **Ready for QA**. Não equivale a aceite nos três sistemas.

## Entrega

- Explorador SFTP por sessão: diretório inicial, pasta pai, caminho atual,
  expansão sob demanda, páginas de 200 entradas e linhas virtualizadas de 30 px.
  Criação exclusiva, renomeação sem substituir e exclusão com revisão/confirmação
  de uso único (120 s). RMDIR não recursivo; links aparecem sem expansão.
- Caminhos e handles SFTP são bytes, identificados por UUID na UI. Escapes são
  somente apresentação; nunca se reconstrói caminho remoto a partir deles.
  Registro limitado a 20.000 caminhos / 8 MiB por sessão, oito cursores de 60 s,
  16 documentos por sessão e 32 abas na interface. Ao atingir limites, fechar
  documentos ou iniciar outra sessão conforme a mensagem apresentada.
- `russh-sftp` 3.0.1 fornece atributos/serialização; suas APIs de paths e handles
  usam String. Foi criado um adaptador de framing SFTP v3 sobre o canal russh para
  preservar bytes não UTF-8. Pacotes de até 1 MiB, blocos de 64 KiB, IDs sequenciais,
  timeout de 30 s, cancelamento e validação de respostas. Não usar conversão lossy
  como identidade. Esse adaptador precisa manter testes de interoperabilidade.
- Monaco com worker local, realce Shell/JavaScript/YAML, busca/substituição,
  undo/redo, Cmd/Ctrl+S, revisão e dirty por modelo; modelos descartados no fechamento.
  Texto UTF-8 até 5 MiB; binários/outros encodings/tamanhos oferecem download.
  BOM e finais LF/CRLF/CR preservados, incluindo mapa por posição em arquivos mistos.
- Revisão: tamanho, mtime e SHA-256. Escrita em temporário irmão, permissões,
  fsync quando negociado e `posix-rename@openssh.com`. Sem extensão, fallback
  destrutivo somente após aviso explícito vinculado à revisão observada. Conflitos
  permitem recarregar, cancelar ou sobrescrever com token. Falhas mantêm o buffer
  dirty e reportam temporário residual quando não foi possível limpá-lo.
- Salvar/descartar/cancelar em aba, sessão, bloqueio, troca de senha e saída.
  Janela e opção Sair passam pelo mesmo guard. Desconectar mantém documentos em
  memória. Cancelar bloqueio por inatividade mantém o cofre aberto; rascunhos
  criptografados e bloqueio automático com recuperação pertencem à task 10.
- Seletores nativos e arquivos soltos na janela autorizam handles locais de uso
  único, com expiração de cinco minutos. A UI confirma o destino do drop. IPC não
  aceita caminhos locais arbitrários. Leituras locais usam descritor autorizado;
  acesso ao disco ocorre fora dos locks globais de estado.
- Fila limitada a 64 operações registradas, até duas ativas por sessão e quatro
  no total. Canais separados do explorador/PTY, progresso agregado a 10 Hz com ACK,
  sequência e geração. Cancelamento alcança espera, rede e cópia; resumos incluem
  erros, ignorados (até 50 nomes) e resíduos. Histórico da UI limitado a 64 itens.
- Downloads usam `.part` exclusivo e só publicam após sucesso. ZIP usa streaming,
  ZIP64, estrutura relativa, rejeição de traversal/duplicatas, limites de 32 níveis
  e 50.000 entradas. Nomes inválidos são escapados em percent encoding sem colisão
  com `%` literal. Links e arquivos especiais são ignorados e reportados.
- Upload usa temporário remoto exclusivo, valida fonte/destino antes de publicar;
  colisões oferecem sobrescrever, renomear ou ignorar, incluindo autorização
  explícita para substituição sem rename POSIX. Atualização da árvore após sucesso.

## Verificações locais

- `npm test`: 22 testes em 11 arquivos, aprovados. Novos casos: virtualização e
  expansão lazy, cancelamento de exclusão, dirty preservado em falha, descarte de
  modelos, token de conflito e abertura atrasada sem trocar contexto de sessão.
- `cargo test --manifest-path src-tauri/Cargo.toml`: 30 aprovados, seis testes
  especiais ignorados por padrão (a integração de arquivos foi executada à parte).
  Novos casos: framing byte a byte com caminhos/nomes/handles não UTF-8, pacote excessivo,
  cancelamento, roundtrip BOM/finais mistos, limite de texto, hash com mesmo
  tamanho/mtime, nomes ZIP seguros e campo ZIP64 em arquivo pequeno.
- `bash tests/fixtures/ssh/run-native.sh --files`: aprovado com OpenSSH/SFTP real,
  em loopback, chaves/Agent/cofre e diretórios temporários próprios. CRUD, nomes
  especiais, tokens de uso único, isolamento de handles, pasta sem permissão,
  pasta não vazia, BOM/CRLF/permissões 0640, alteração externa e sobrescrita,
  remoção externa com conflito e recriação autorizada, falha de gravação em pasta
  somente leitura preservando original, binário e limite de tamanho.
- Transferências reais: download de 8 MiB, ZIP com subpasta e link ignorado,
  upload por renomeação, sobrescrever/ignorar colisões, duas operações bloqueadas
  em colisões com terceira na fila e cancelamento sem publicar resultado parcial.
- Build TypeScript/Vite, cargo check/fmt/test e bundle debug `.app` executados.
  Vite mantém aviso de chunk Monaco acima de 500 kB, carregado sob demanda.
- Smoke WebView macOS empacotado, CSP ativa: desbloqueio de cofre descartável,
  conexão real, árvore SFTP, abrir/editar/salvar no Monaco, indicador dirty limpo
  após sucesso, seletor nativo de download, resultado concluído, seletor de upload,
  resultado remoto na árvore. Bloquear com dirty abriu diálogo; cancelar preservou
  conteúdo. Fechar janela ofereceu salvar todos e encerrou após a gravação.
  Cmd+Q foi repetido após substituir o Quit predefinido Cocoa por item próprio:
  abriu o guard, cancelar preservou o texto e salvar todos encerrou após sucesso.

## Limitações e QA pendente

- Sem runners/remoto Git neste workspace. Windows/Linux e seus seletores, ACLs,
  workers/CSP, interação e cenários de erro ainda precisam de execução nativa.
  CI acrescenta integração de arquivos nos runners macOS/Linux; não foi disparada.
- APFS local recusou criar nome não UTF-8. Preservação foi comprovada no protocolo
  por teste duplex; a fixture faz CRUD real com esse nome quando o FS permite.
  O teste de filesystem fica pendente no Linux, não é contabilizado como aprovado
  no macOS.
- Falta servidor realmente sem extensão POSIX rename, queda física durante escrita,
  falta de espaço real, arquivo >4 GiB e arquivo ZIP com >65.535 entradas. ZIP64
  foi comprovado por formato, não por transferência de arquivo multigigabyte.
- Arrastar/soltar pelo sistema operacional e fluxo completo de colisão na UI,
  múltiplas abas, busca/substituição e ações de conflito precisam de QA adicional.
  Cobertura de serviço ou mocks não substitui esse aceite.
- SFTP v3 não tem CAS ou O_NOFOLLOW universal. Há comparação antes/depois da
  leitura e antes de publicar, lstat/fstat e canonicalização de diretórios. Ainda
  há janela residual de alteração externa entre checagem e operação, incluindo
  troca concorrente de caminho por link. Não prometer snapshot transacional nem
  proteção absoluta contra um servidor remoto malicioso.
- Após falha de transporte/timeout, o canal de navegação é inutilizado para evitar
  respostas fora de ordem. Abra nova sessão; o PTY e buffers locais são independentes.
- Downloads `.part` e temporários remotos cuja limpeza falhou podem exigir remoção
  manual após conferir o resumo. Encerramento abrupto pode impedir o resumo final.
  Nenhum parcial é apresentado como concluído.
