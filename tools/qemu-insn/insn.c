/* Counts guest instructions executed; prints the total at exit. */
#include <stdio.h>
#include <inttypes.h>
#include <qemu-plugin.h>

QEMU_PLUGIN_EXPORT int qemu_plugin_version = QEMU_PLUGIN_VERSION;

static struct qemu_plugin_scoreboard *counts;
static qemu_plugin_u64 count;

static void tb_trans(struct qemu_plugin_tb *tb, void *ud)
{
    qemu_plugin_register_vcpu_tb_exec_inline_per_vcpu(
        tb, QEMU_PLUGIN_INLINE_ADD_U64, count, qemu_plugin_tb_n_insns(tb));
}

static void at_exit(void *p)
{
    fprintf(stderr, "guest_insns %" PRIu64 "\n", qemu_plugin_u64_sum(count));
}

QEMU_PLUGIN_EXPORT int qemu_plugin_install(qemu_plugin_id_t id, const qemu_info_t *info,
                                           int argc, char **argv)
{
    counts = qemu_plugin_scoreboard_new(sizeof(uint64_t));
    count = qemu_plugin_scoreboard_u64(counts);
    qemu_plugin_register_vcpu_tb_trans_cb(id, tb_trans, NULL);
    qemu_plugin_register_atexit_cb(id, at_exit, NULL);
    return 0;
}
