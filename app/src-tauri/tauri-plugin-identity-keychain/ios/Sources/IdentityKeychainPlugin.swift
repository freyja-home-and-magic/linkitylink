import Foundation
import Security
import Tauri
import UIKit

// The keychain group every HomeFront app shares. The entitlement is
// "$(AppIdentifierPrefix)club.home.front" (build-ios.cjs and dev-ios.cjs
// write it); at runtime the group needs the expanded team prefix.
private let accessGroup = "PCUFMY9NKZ.club.home.front"
// One item per allyabase service, told apart by account.
private let itemService = "club.home.front.identity"

struct ServiceArgs: Decodable {
    let service: String
}

struct WriteArgs: Decodable {
    let service: String
    let value: String
}

private func baseQuery(_ account: String) -> [String: Any] {
    return [
        kSecClass as String: kSecClassGenericPassword,
        kSecAttrService as String: itemService,
        kSecAttrAccount as String: account,
        kSecAttrAccessGroup as String: accessGroup,
        kSecAttrSynchronizable as String: false,
    ]
}

private func describe(_ status: OSStatus) -> String {
    let message = SecCopyErrorMessageString(status, nil) as String? ?? "unknown"
    return "keychain error \(status): \(message)"
}

@objc public class IdentityKeychainPlugin: Plugin {

    @objc public func read(_ invoke: Invoke) throws {
        let args = try invoke.parseArgs(ServiceArgs.self)
        var query = baseQuery(args.service)
        query[kSecReturnData as String] = true
        query[kSecMatchLimit as String] = kSecMatchLimitOne
        var result: AnyObject?
        let status = SecItemCopyMatching(query as CFDictionary, &result)
        switch status {
        case errSecSuccess:
            guard let data = result as? Data, let value = String(data: data, encoding: .utf8) else {
                invoke.reject("keychain item for \(args.service) isn't text")
                return
            }
            invoke.resolve(["value": value])
        case errSecItemNotFound:
            invoke.resolve(["value": NSNull()])
        default:
            invoke.reject(describe(status))
        }
    }

    // Adds only if nothing is there: { added: false } means another app got
    // there first, and the caller reads theirs.
    @objc public func add(_ invoke: Invoke) throws {
        let args = try invoke.parseArgs(WriteArgs.self)
        var query = baseQuery(args.service)
        query[kSecValueData as String] = Data(args.value.utf8)
        // Readable once the phone has been unlocked since boot, so the apps
        // can earn in the background; travels in encrypted backups.
        query[kSecAttrAccessible as String] = kSecAttrAccessibleAfterFirstUnlock
        let status = SecItemAdd(query as CFDictionary, nil)
        switch status {
        case errSecSuccess:
            invoke.resolve(["added": true])
        case errSecDuplicateItem:
            invoke.resolve(["added": false])
        default:
            invoke.reject(describe(status))
        }
    }

    @objc public func update(_ invoke: Invoke) throws {
        let args = try invoke.parseArgs(WriteArgs.self)
        let changes: [String: Any] = [kSecValueData as String: Data(args.value.utf8)]
        let status = SecItemUpdate(baseQuery(args.service) as CFDictionary, changes as CFDictionary)
        if status == errSecSuccess {
            invoke.resolve()
        } else {
            invoke.reject(describe(status))
        }
    }

    @objc public func remove(_ invoke: Invoke) throws {
        let args = try invoke.parseArgs(ServiceArgs.self)
        let status = SecItemDelete(baseQuery(args.service) as CFDictionary)
        if status == errSecSuccess || status == errSecItemNotFound {
            invoke.resolve()
        } else {
            invoke.reject(describe(status))
        }
    }
}

@_cdecl("init_plugin_identity_keychain")
func initPlugin() -> Plugin {
    return IdentityKeychainPlugin()
}
