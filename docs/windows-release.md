# Release Windows pelo GitHub Actions

O workflow [Windows release](../.github/workflows/release-windows.yml) executa
ao enviar uma tag como `v0.1.0`. Ele valida a versão, executa os testes frontend e
Rust, verifica a formatação e compila um instalador NSIS `*-setup.exe` de produção
para Windows x64 em `windows-latest`. O build do Tauri também compila o frontend.

Depois que o build termina, outro job publica o instalador na release dessa tag.
Se a release não existir, ela é criada como rascunho, recebe o instalador e é
publicada automaticamente, com notas geradas pelo GitHub. Se já existir, seu
texto e outros anexos (por exemplo, o DMG) são preservados. Uma release existente
em rascunho também será publicada. Reexecutar o workflow substitui somente o
anexo com o mesmo nome. Tags como `v0.2.0-beta.1` criam uma pré-release.

## Publicar

Antes de criar a tag, confirme que o commit inclui o workflow e todos os ajustes
que deverão entrar no instalador. As versões de `package.json`, `package-lock.json`
(raiz e pacote raiz), `src-tauri/Cargo.toml` e `src-tauri/tauri.conf.json` devem
coincidir com a tag sem o prefixo `v`. Mantenha também o `Cargo.lock` atualizado.

Para a versão 0.1.0, após fazer commit das alterações:

```sh
git push origin HEAD
git tag -a v0.1.0 -m "Release 0.1.0"
git push origin v0.1.0
```

Use esse exemplo somente se a tag ainda não existir. Para outra versão, atualize
os arquivos de versão antes do commit e use uma nova tag. Enviar uma tag existente
sem alterações não dispara um novo build. Para repetir um build, abra
**Actions → Windows release → execução → Re-run all jobs**.

No GitHub, acompanhe **Actions → Windows release**. Após a conclusão, baixe o
`*-setup.exe` em **Releases → versão → Assets**. O instalador também fica como
artifact `windows-x64-installer` da execução por 14 dias, para diagnóstico.

Não é necessário criar um token pessoal: a publicação usa o `GITHUB_TOKEN`
automático, com `contents: write` somente no job de publicação. O repositório deve
permitir GitHub Actions e essa permissão; políticas da organização podem limitá-la.
O workflow não envia commits, não cria tags e não publica apenas ao enviar a branch.

## Limitações

O instalador não tem assinatura Authenticode configurada e pode exibir avisos do
SmartScreen/editor desconhecido. Nenhum certificado ou segredo adicional é exigido
para este build. Um build bem-sucedido não substitui testar instalação, abertura,
cofre, SSH e transferência no Windows. ARM64 e MSI não são gerados neste workflow.

Referências: [pipeline do Tauri](https://v2.tauri.app/distribute/pipelines/github/)
e [instaladores Windows](https://v2.tauri.app/distribute/windows-installer/).
