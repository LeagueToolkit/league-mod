#!/bin/bash

cat <<'EOF' > openssl.cnf
[ ca ]
default_ca = CA_default
intermediate_ca = intermediate_ca

[ CA_default ]
dir               = .
certs             = $dir/certs
new_certs_dir     = $dir/newcerts
database          = $dir/index.txt
serial            = $dir/serial
private_key       = $dir/rootCA.key
certificate       = $dir/rootCA.pem
default_md        = sha512
policy            = policy_anything
email_in_dn       = no
name_opt          = ca_default
cert_opt          = ca_default
copy_extensions   = copy
default_days      = 3650
crl_days          = 3650
crl_extensions    = crl_ext
unique_subject    = no
default_crl_days = 3650

[ intermediate_ca ]
dir               = ./intermediate
certs             = $dir/certs
new_certs_dir     = $dir/newcerts
database          = $dir/index.txt
serial            = $dir/serial
private_key       = $dir/intermediate.key
certificate       = $dir/intermediate.pem
default_md        = sha512
policy            = policy_anything
email_in_dn       = no
name_opt          = ca_default
cert_opt          = ca_default
copy_extensions   = copy
default_days      = 1825
crl_days          = 30
crl_extensions    = crl_ext
unique_subject    = no
default_crl_days  = 30

[ policy_anything ]
countryName             = optional
stateOrProvinceName     = optional
organizationName        = optional
organizationalUnitName  = optional
commonName              = supplied
emailAddress            = optional

[ req ]
default_bits        = 2048
default_md          = sha512
distinguished_name  = req_distinguished_name
x509_extensions     = v3_ca
string_mask         = utf8only

[ req_distinguished_name ]
countryName                     = Country Name (2 letter code)
stateOrProvinceName             = State or Province Name
localityName                    = Locality Name
0.organizationName              = Organization Name
organizationalUnitName          = Organizational Unit Name
commonName                     = Common Name
emailAddress                   = Email Address

[ v3_ca ]
basicConstraints = critical, CA:true
keyUsage = critical, digitalSignature, cRLSign, keyCertSign
subjectKeyIdentifier = hash
authorityKeyIdentifier = keyid:always,issuer

[ crl_ext ]
authorityKeyIdentifier=keyid:always


[ codesign_ext ]
basicConstraints = CA:FALSE
keyUsage = digitalSignature, nonRepudiation
extendedKeyUsage = codeSigning, emailProtection
subjectKeyIdentifier = hash
authorityKeyIdentifier = keyid,issuer
EOF

mkdir -p ./{certs,newcerts}
touch index.txt
echo 1000 > serial
openssl genrsa -out rootCA.key 2048
MSYS_NO_PATHCONV=1 openssl req -x509 -new -nodes -key rootCA.key -sha512 -days 3650 -out rootCA.pem -subj "/C=US/ST=CA/L=City/O=MyOrg/OU=RootCA/CN=MyRootCA" -extensions v3_ca -reqexts v3_req
openssl ca -batch -config openssl.cnf -keyfile rootCA.key -cert rootCA.pem -gencrl -out rootCA.crl

mkdir -p intermediate/{certs,newcerts}
touch intermediate/index.txt
echo 2000 > intermediate/serial
openssl genrsa -out intermediate/intermediate.key 2048
MSYS_NO_PATHCONV=1 openssl req -new -sha512 -key intermediate/intermediate.key -out intermediate/intermediate.csr -subj "/C=US/ST=CA/L=City/O=MyOrg/OU=IntermediateCA/CN=MyIntermediateCA"
openssl ca -batch -config openssl.cnf -extensions v3_ca -days 1825 -notext -md sha512 -in intermediate/intermediate.csr -out intermediate/intermediate.pem
openssl ca -batch -config openssl.cnf -name intermediate_ca -keyfile intermediate/intermediate.key -cert intermediate/intermediate.pem -gencrl -out intermediate/intermediate.crl

openssl genrsa -out codesign.key 2048
MSYS_NO_PATHCONV=1  openssl req -new -key codesign.key -out codesign.csr -subj "/C=US/ST=CA/L=City/O=MyOrg/OU=TestSigning/CN=Test Code Signing"
openssl ca -batch -config openssl.cnf -name intermediate_ca -extensions codesign_ext -days 730 -notext -md sha512 -in codesign.csr -out codesign.pem

cat codesign.key > test.pem
cat codesign.pem >> test.pem
cat intermediate/intermediate.key >> test.pem
cat intermediate/intermediate.pem >> test.pem
cat intermediate/intermediate.crl >> test.pem
cat rootCA.key >> test.pem
cat rootCA.pem >> test.pem
cat rootCA.crl >> test.pem
