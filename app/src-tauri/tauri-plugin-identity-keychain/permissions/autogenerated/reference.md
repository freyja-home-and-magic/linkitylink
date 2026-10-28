## Default Permission

Default permissions for identity-keychain. Lets the web view read the PUBLIC half of an identity (pubKey and uuids); private keys never leave Rust.

#### This default permission set includes the following:

- `allow-public-identity`

## Permission Table

<table>
<tr>
<th>Identifier</th>
<th>Description</th>
</tr>


<tr>
<td>

`identity-keychain:allow-public-identity`

</td>
<td>

Enables the public_identity command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`identity-keychain:deny-public-identity`

</td>
<td>

Denies the public_identity command without any pre-configured scope.

</td>
</tr>
</table>
