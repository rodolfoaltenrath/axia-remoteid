# Contrato recuperado do RemoteID

Esta referência conserva os resultados necessários à reimplementação independente. O pseudocódigo do Ghidra não é código C++ compilável. Os instaladores, extrações e projetos da investigação não são distribuídos neste repositório; os endereços abaixo permitem reconhecer a origem dos achados se uma nova investigação for necessária.

## Rotas e autenticação

Origem validada para a conta testada: `https://remoteidcertisign.com.br`. `urlAcesso` aparece somente no corpo do login; não é acrescentado à URL. Login/cadastro foram aceitos com `urlAcesso` vazio e `sistemaOperacional=Linux`.

| Operação | Rota | Corpo / resultado usado |
| --- | --- | --- |
| Login POST | `/api/manager/usuarios/login/usrsenha` | `email`, `urlAcesso`, `senha` → `id`, `organizacaoId`, `token` |
| Cadastro POST | `/api/manager/desktopid/usuario/{user}/organizacao/{org}` | `nomeDesktop`, `sistemaOperacional`, `nomeUsuarioLocal`, `dominioRede`, `chavePublica` → `codigoDesktop` |
| Carteira POST | `/api/manager/desktopid/{desktop}/carteira` | `momento` como segundos Unix em string → `certificados`, com `base64` DER e `keyName` |
| Sessão POST | `/api/signature/tokensessao` | `desktopCode`, `pin`, `otp`, `push`, `nomeAplicacaoDesktop`, `issue`, `serialNumber` → `status`, `token` |
| Assinatura POST | `/api/signature/requestHashSessionSignature` | `desktopCode`, `sessionToken`, `issue`, `serialNumber`, `algorithm`, `hashArray` → `status`, `idArray` |
| Perfil GET | `/api/manager/usuarios/visualizarproprio/{user}/{org}` | Diagnóstico de autenticação, sem cadastro |

O cadastro original usa `Authorization: Bearer<token>`, **sem espaço**. Pedidos assinados usam `Authorization: Bearer <assinaturaBase64>`, com espaço. A consulta de perfil aceitou ambas as grafias com o token de login; isso não define o contrato das demais rotas.

Quando o domínio está vazio, o original substitui por `Local`. O cliente foi corrigido após duas respostas HTTP 500 com domínio vazio; a tentativa seguinte, preservando a identidade e usando `Local`, foi aceita. Isso não demonstra idempotência nem permite afirmar o estado remoto das tentativas anteriores. CertiNext pertence ao bootstrap MobileID e não é pré-condição recuperada de `createRemoteIdConfiguration`.

## Identidade e assinatura dos pedidos

A identidade da instalação é RSA-2048, com chave privada PKCS#8 e chave pública SPKI em PEM. Ela autentica os pedidos; é diferente da chave privada do certificado em nuvem. Não é possível exportar essa chave remota pelo módulo.

Canonicalização compatível com o original:

1. Ordenar as chaves de cada objeto.
2. Concatenar valores string em UTF-8, sem separadores ou escapes JSON.
3. Para `true`, concatenar o nome da chave; `false` e `null` não contribuem.
4. Inteiros assinados de 64 bits contribuem em decimal.
5. Objetos são percorridos recursivamente; arrays de objetos preservam sua ordem.
6. O cliente rejeita floats, inteiros fora desse intervalo e arrays escalares sem contrato comprovado.

Assinar essa sequência com RSA PKCS#1 v1.5/SHA-256 da instalação e codificar em Base64. Isso produz a autorização dos pedidos de carteira, sessão e assinatura. Os vetores públicos de interoperabilidade comparam o modelo Python independente e o Rust.

## Referência do certificado e resposta de assinatura

`keyName` da carteira é dividido em `;`, comprimindo separadores vazios como no original. Primeiro componente → `serialNumber`; segundo → `issue`. O cliente exige dois componentes ASCII reconhecidos. Não derivar esses campos da apresentação do emissor ou da série no X.509, cuja ordem/formatação é diferente.

A sessão usa a senha do aplicativo Certisign no campo `pin` e o OTP como string, preservando zeros iniciais. O pedido de assinatura envia o digest bruto em Base64, em `hashArray=[{"id":0,"hash":"..."}]`; o ID do pedido é inteiro. Algoritmos do protocolo: `SHA256`, `SHA384`, `SHA512` (SHA-1 existe no material original, mas não é oferecido operacionalmente nesta versão).

Exigir status global e individual verdadeiros, exatamente um resultado e `signatureBase64` válida e não vazia. **Não exigir ID numérico 0 na resposta**: o leitor original não consulta esse campo e a conta real respondeu com ID string. O valor não foi preservado. Antes de aceitar o resultado, verificar RSA PKCS#1 v1.5 contra o digest e o certificado selecionado. Texto alterado deve falhar.

O original guarda sessão por 30 minutos no cliente. TTL do servidor, códigos de erro, cancelamento remoto e aceitação real de SHA-384/512 continuam desconhecidos. O cliente atual não reutiliza tokens entre assinaturas. As rotas adicionais `/api/manager/desktopid/{desktop}/carteira/invalida` e `/api/manager/desktopid/{desktop}/statusCelular` foram encontradas, mas não integram o fluxo atual; seus contratos completos não foram validados. O 304 da carteira original depende de `If-Modified-Since`; não tratar como sucesso vazio de uma consulta incondicional.

## Interface PKCS#11

ABI 2.40, slot 8, token somente leitura. IDs dos objetos são SHA-256 do certificado DER. Certificados e chaves públicas são locais; objetos privados são referências sensíveis e não extraíveis. `C_Login(CKU_USER, NULL, 0)` habilita referências pelo caminho de autenticação protegido, sem pedido remoto. PIN passado pelo chamador é rejeitado.

`CKM_SHA256_RSA_PKCS`, `CKM_SHA384_RSA_PKCS` e `CKM_SHA512_RSA_PKCS` recebem a mensagem. `CKM_RSA_PKCS` aceita somente DigestInfo DER desses três algoritmos. Consultas de tamanho e buffers curtos preservam a operação sem auxiliar nem assinatura; a chamada efetiva consome uma vez e verifica o resultado antes de devolvê-lo. O mutex global é liberado durante o auxiliar; cada sessão conserva seu estado. Limites atuais: 64 sessões, mensagem multipart de 16 MiB e auxiliar de 400 segundos. Operações não implementadas devolvem erro explícito.

O auxiliar recebe somente impressão digital, algoritmo e digest por stdin, com protocolo JSON limitado e campos desconhecidos rejeitados. Senha/OTP são obtidos na janela Zenity, nunca por argv, variável de ambiente, IPC ou disco. Os dois pedidos de rede não têm repetição automática. Módulo e auxiliar verificam a assinatura. Cancelamento encerra a operação. Nenhuma dessas garantias permite validar ponteiros arbitrários fornecidos por um chamador C; a ABI exige buffers válidos.

Referências: [OASIS PKCS#11 2.40](https://docs.oasis-open.org/pkcs11/pkcs11-base/v2.40/pkcs11-base-v2.40.html) e [cryptoki-sys 0.5.0](https://docs.rs/cryptoki-sys/0.5.0/cryptoki_sys/).

## Origem dos achados estáticos

Endereços do provedor macOS Intel 2.2.0.1: canonicalização `0x11fab`, assinatura local `0x121fa`, Bearer assinado `0x12616`, configuração RemoteID `0x4e22c`, login `0x4e3f5`, cadastro `0x4ee4c`, origem `0x500b0`, carteira `0x504c8`, sessão `0x60ed2`, signHash `0x61c50`, cache `0x4b042`/`0x4b180`. Request ID inteiro: `0x61f51`, chamada `Json::Value(int)` em `0x23508`; leitor sem consulta ao ID: `0x62174..0x621f2`.

Literais de autorização: `L"Bearer "` em `0x294b9c` e `L"Bearer"` em `0x296f18`. A assimetria do cadastro também foi conferida no GUI Windows 2.1.4.3. Fallback de domínio no GUI macOS: `InstallationModel::domainNameLeftBlank` em `0x10000d5e0`, literal UTF32LE `Local` em `0x1002c8bf4`.

SHA-256 dos instaladores utilizados na investigação:

```text
SetupDesktopID.exe  3c0cddb0decc845f1e80269b848c1e1c1933a14867d2619a5fd8df65c3425023
desktopID_Intel.pkg 6a9d3790f8e6e9540133f7c7e987b0ab49b35eb52e66386b8045721750bd43f8
```

São identificadores históricos; os arquivos proprietários não fazem parte da distribuição. Os achados documentados e os testes de laboratório permanecem disponíveis, mas a árvore não conserva a desassemblagem completa.
