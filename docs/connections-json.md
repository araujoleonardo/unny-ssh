# Importar e exportar conexões JSON

Use **Importar JSON** e **Exportar JSON** na lateral de conexões, ou as mesmas
opções no dropdown da barra superior. O cofre precisa estar desbloqueado.
A seleção do arquivo e do destino usa os diálogos nativos do sistema.

O formato é `ssh-client-backup`, versão `1`, conforme o
[arquivo de referência](ssh-sessions-2026-11-06.json): `exportedAt`, `note`,
`folders` e `connections`, com os nomes de campos em camelCase.

- Conexões preservam ID, nome, host, porta, usuário, `authType`, ícone,
  `keyFileName`, `folderId`, `createdAt` e `updatedAt`.
- Pastas preservam ID, nome e `createdAt` para uma posterior exportação. A lista
  de conexões do aplicativo continua sem agrupamento em pastas.
- O diretório inicial do unny-ssh, quando definido, usa o campo opcional
  `initialDirectory`. Backups sem esse campo continuam compatíveis.
- Os tipos de autenticação aceitos são `password`, `key` e `agent`.
  `keyFileName` é apenas uma indicação do nome, sem ler ou importar esse arquivo.

## Importação

O aplicativo valida todo o documento e apresenta uma confirmação com a quantidade
de conexões e pastas. A importação é aditiva: IDs de conexões já existentes são
ignorados, preservando todos os dados e credenciais locais. Pastas com IDs já
existentes também são preservadas sem substituição. Conexões com IDs diferentes
são independentes, mesmo quando têm nomes ou endereços iguais.

Campos de senha, passphrase, chave privada ou referências de segredo presentes no
JSON são ignorados. As conexões importadas ficam **sem credenciais salvas**;
informe-as ao conectar, ou selecione novamente a chave privada. SSH Agent consulta
o agente do sistema. Identidades de servidores confiáveis não são importadas:
a confirmação da impressão digital continua sendo exigida normalmente.

JSON inválido, versão incompatível, IDs duplicados dentro do arquivo, referência
para pasta ausente, host/porta inválidos ou excesso de limites impedem o lote
inteiro. Uma falha ao persistir também preserva o cofre anterior. Cancelar os
diálogos não altera as conexões. Bloquear o cofre durante o fluxo invalida a
operação pendente.

Limites: arquivo de até **2 MiB**, **1000 conexões** e **1000 pastas** no cofre.
O arquivo deve usar UTF-8, com ou sem BOM.

## Exportação

O exportador monta uma lista explícita de campos permitidos, sem serializar o
cofre ou os segredos. **Senhas, passphrases e chaves privadas nunca são incluídas**,
assim como referências internas de credenciais, sessões e identidades confiáveis.
A data `exportedAt` é gerada em UTC. A gravação usa substituição atômica; no Unix,
o arquivo é criado com permissões 0600.

Conexões antigas sem histórico de criação recebem datas do momento da exportação.
As datas de conexões importadas são preservadas; editar uma conexão atualiza sua
data de modificação.

O JSON exportado não é criptografado: nomes, hosts e usuários continuam legíveis.
Ele não é um backup completo do cofre.

## Validação da implementação

- 39 testes frontend e 48 testes Rust aprovados (11 Rust dependentes de outras
  fixtures ignorados). Incluem roundtrip, arquivo fornecido, IDs repetidos,
  preservação das credenciais existentes, dados inválidos, falha real de escrita,
  gerações do cofre, payloads antigos e exclusão de campos secretos.
- `npm run build`, `cargo check` e `cargo fmt -- --check` aprovados.
- Fluxo nativo no macOS, em cofre descartável: confirmação e importação das
  13 conexões/3 pastas do arquivo fornecido; exportação por diálogo nativo e
  comparação do arquivo gravado. Os registros importados e as pastas coincidiram
  com a referência. O perfil adicional da fixture com chave salva foi exportado
  sem chave privada, e o JSON tinha permissões 0600.
- `.app` release Apple Silicon recompilado por `npm run build:macos:app:local`;
  assinatura ad hoc completa validada por `codesign --verify --deep --strict`.

Os testes nativos de diálogos e arquivos desta funcionalidade ainda precisam ser
repetidos no Windows/Linux. Nenhuma conexão do arquivo de referência foi aberta
na validação; os testes usaram somente um cofre temporário, removido ao terminar.
