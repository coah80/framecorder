// framecorder.exe: what frame drop launches from the steam library.
//
// frame drop can't start linux programs: it uploads everything over sftp from
// windows, so nothing arrives executable and steam gets "permission denied".
// a windows exe is the one thing it launches that doesn't need that, since
// steam runs it through proton. so this is all it is: under proton, wine can
// start linux programs, and /bin/sh is always executable, so it runs
// install.sh (which gets out of steam's container and runs the real
// installer) and waits for it to finish.
//
// built by packaging/framedrop/build.sh.

#include <windows.h>
#include <stdio.h>
#include <string.h>

#define WAIT_SECS 180

// wine exports this from kernel32 to turn a windows path into a linux one
typedef char *(CDECL *unix_name_fn)(const WCHAR *);

static void note(const WCHAR *dir, const char *text) {
    WCHAR path[MAX_PATH + 32];
    _snwprintf(path, MAX_PATH + 32, L"%ls\\launcher.log", dir);
    FILE *f = _wfopen(path, L"a");
    if (f) {
        fprintf(f, "%s\n", text);
        fclose(f);
    }
}

int main(void) {
    WCHAR dir[MAX_PATH];
    DWORD len = GetModuleFileNameW(NULL, dir, MAX_PATH);
    if (len == 0 || len >= MAX_PATH) return 1;
    WCHAR *slash = wcsrchr(dir, L'\\');
    if (!slash) return 1;
    *slash = 0;

    unix_name_fn unix_name = (unix_name_fn)GetProcAddress(GetModuleHandleW(L"kernel32"), "wine_get_unix_file_name");
    if (!unix_name) {
        note(dir, "not running under proton, nothing to do");
        return 1;
    }
    char *unix_dir = unix_name(dir);
    if (!unix_dir) {
        note(dir, "couldn't find this folder on the headset");
        return 1;
    }

    WCHAR done[MAX_PATH + 32];
    _snwprintf(done, MAX_PATH + 32, L"%ls\\install.done", dir);
    DeleteFileW(done);

    WCHAR cmd[MAX_PATH * 2];
    _snwprintf(cmd, MAX_PATH * 2, L"/bin/sh \"%hs/install.sh\"", unix_dir);
    HeapFree(GetProcessHeap(), 0, unix_dir);

    STARTUPINFOW si = { sizeof si };
    PROCESS_INFORMATION pi;
    if (!CreateProcessW(L"Z:\\bin\\sh", cmd, NULL, NULL, FALSE, 0, NULL, NULL, &si, &pi)) {
        char msg[64];
        snprintf(msg, sizeof msg, "couldn't start /bin/sh (error %lu)", GetLastError());
        note(dir, msg);
        return 1;
    }
    CloseHandle(pi.hThread);
    CloseHandle(pi.hProcess);

    // wine can't wait on a linux process, so install.sh leaves a file when it's done
    for (int i = 0; i < WAIT_SECS * 4; i++) {
        if (GetFileAttributesW(done) != INVALID_FILE_ATTRIBUTES) return 0;
        Sleep(250);
    }
    note(dir, "gave up waiting for install.sh");
    return 1;
}
