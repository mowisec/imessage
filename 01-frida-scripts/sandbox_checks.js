/*
    This script shows which sandbox checks are performed by a process.
    E.g., launchd checks which mach lookups are permitted, which allows 
    to follow new XPC connection establishment.

    `frida -U launchd -l xpc_connections.js`
*/

const sandbox_check_by_audit_token_addr = Module.getExportByName(null, 'sandbox_check_by_audit_token');
const sandbox_check_addr = Module.getExportByName(null, 'sandbox_check');

const audit_token_to_pid_addr = Module.getExportByName(null, 'audit_token_to_pid');
const audit_token_to_pid = new NativeFunction(audit_token_to_pid_addr, 'uint32', ['pointer']); // audit_token is an int[8]

const proc_name_addr = Module.getExportByName(null, 'proc_name');
const proc_name = new NativeFunction(proc_name_addr, 'void', ['uint32', 'pointer', 'uint32']); // int pid, char *buf, int size

Interceptor.attach(sandbox_check_by_audit_token_addr, {
    onEnter: function(args) {
        // convert audit token to process ID 
        let pid = audit_token_to_pid(args[0]);

        // convert pid to process name
        let mem = Memory.alloc(0x100);
        proc_name(pid, mem, 0x100);
        let name = mem.readCString();
        
        // get permission name
        let permission = 'NULL';
        if (parseInt(args[1]) != 0) {
            permission = args[1].readCString();
        }

        // flags are passed as integer
        let flags = args[2];
        
        // for mach-lookup, the third parameter is the service name.
        // but it may also contain an integer instead of a pointer.
        let service = parseInt(args[3]);
        if (service > 0x100000000) { // lazy check if this is a pointer
            service = args[3].readCString()
        }

        console.log(`${name}[${pid}], ${permission}(${flags}): ${service}`);
    }
});

// If used with null args, this is used to check our own process
// and see if it's sandboxed.
Interceptor.attach(sandbox_check_addr, {
    onEnter: function(args) {
        // first argument is pid 
        let pid = parseInt(args[0]);

        // convert pid to process name
        let mem = Memory.alloc(0x100);
        proc_name(pid, mem, 0x100);
        let name = mem.readCString();
        
        // get operation name
        let permission = 'NULL';
        if (parseInt(args[1]) != 0) {
            permission = args[1].readCString();
        }

        // sandbox filter type, varargs follow
        let type = args[2];

        // check if first vararg is a string
        let service = parseInt(args[3]);
        if (service > 0x100000000) { // lazy check if this is a pointer
            service = args[3].readCString()
        }

        console.log(`${name}[${pid}], ${permission}(${type}): ${service}`);
    }
});