import { scryptSync, createHash, createHmac } from 'node:crypto';
import { writeFileSync } from 'node:fs';
import { verifyPassword, symmetricDecrypt } from 'better-auth/crypto';
import { createOTP } from '@better-auth/utils/otp';
import { xchacha20poly1305 } from '@noble/ciphers/chacha.js';
import { assessEnrolment } from '../../src/lib/enrolment';

// Only synthetic credentials. Fixed nonce/salt make the compatibility fixture reproducible.
const authSecret = 'identity-fixture-secret-longer-than-32-bytes';
const passwords = [];
for (const password of ['PasswordForTest123!', 'ＰａｓｓｗｏｒｄForTest１２３!', '密碼ForTest123!']) {
  const salt='0123456789abcdef0123456789abcdef';
  const hash=`${salt}:${scryptSync(password.normalize('NFKC'),salt,64,{N:16384,r:16,p:1,maxmem:67108864}).toString('hex')}`;
  if (!await verifyPassword({hash,password})) throw new Error('Original password verifier rejected fixture');
  passwords.push({password,hash});
}
const plaintext='SyntheticTotpSecret0123456789ABCD';
const nonce=Uint8Array.from({length:24},(_,i)=>i);
const key=createHash('sha256').update(authSecret).digest();
const encrypted=xchacha20poly1305(key,nonce).encrypt(new TextEncoder().encode(plaintext));
const factor=Buffer.concat([nonce,encrypted]).toString('hex');
if (await symmetricDecrypt({key:authSecret,data:factor})!==plaintext) throw new Error('Original cipher rejected fixture');
const otp=[];
for(const seconds of [59,1111111109,1234567890,1790812800]) otp.push({seconds,code:await createOTP(plaintext).hotp(Math.floor(seconds/30))});
const enrolments=[];
for(const owner of [false,true]) for(const password of [false,true]) for(const twoFactor of [false,true]) for(const passkeys of [0,1,2]) for(const providers of [[],['credential'],['steam'],['discord','steam']]) for(const recoveryKey of [false,true]) {
  const methods={password,twoFactor,passkeys,providers,recoveryKey};
  enrolments.push({owner,methods,result:assessEnrolment(methods,owner?'owner':'member')});
}
const cookieToken='fixture-session-token';
const cookie=`warcon.session_token=${encodeURIComponent(`${cookieToken}.${createHmac('sha256',authSecret).update(cookieToken).digest('base64')}`)}`;
writeFileSync(new URL('./identity.json',import.meta.url),JSON.stringify({authSecret,passwords,plaintext,factor,otp,enrolments,cookie,cookieToken},null,2)+'\n');
console.log(JSON.stringify({passwords:passwords.length,otp:otp.length,enrolments:enrolments.length}));
