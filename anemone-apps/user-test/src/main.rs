#![no_std]
#![no_main]

use core::{ffi::CStr, panic::PanicInfo};

use anemone_abi::syscall::syscall;

#[unsafe(no_mangle)]
pub fn _start() {
    unsafe {
        let str = c"hello_world".as_ptr();
        syscall(100, str as u64, 0, 0, 0, 0, 0);

        let str = c"测试一下中文".as_ptr();
        syscall(100, str as u64, 0, 0, 0, 0, 0);

        let str = c"冬の花（冬之花）
歌手：宮本浩次（日剧《后妻业》主题曲）作词 / 作曲：宮本浩次
いずれ花と散る わたしの生命终将如花凋零 我的生命izure hana to chiru watashi no inochi
帰らぬ時 指おり数えても细数一去不返的时光kaeranu toki yubi ori kazoete mo
涙と笑い 過去と未来泪与笑 过去与未来namida to warai kako to mirai
引き裂かれしわたしは 冬の花被撕裂的我 是冬之花hikisakare shi watashi wa fuyu no hana
あなたは太陽 わたしは月你是太阳 我是月亮anata wa taiyou watashi wa tsuki
光と闇が交じり合わぬように仿佛光与影永不相交hikari to yami ga majirawanu you ni
涙にけむる ふたりの未来泪眼朦胧 我们的未来namida ni kemuru futari no mirai
美しすぎる過去は蜃気楼太过美好的过往 不过是海市蜃楼utsukushisugiru kako wa shinkirou
旅みたいだね人生就像一场旅行啊tabi mitai da ne
生きるってどんな時でも活着 无论何时ikiru tte donna toki demo
木枯らしの中在寒风之中kogarashi no naka
ぬくもり求め 彷徨う追寻温暖 彷徨无措nukumori motome samayou
泣かないで わたしの恋心别哭泣啊 我的爱恋之心nakanaide watashi no koigokoro
涙はお前には似合わない眼泪 与你并不相配namida wa omae ni wa niawanai
ゆけ ただゆけ走吧 只管向前走yuke tada yuke
いっそわたしがゆくよ索性 我便独自前行isso watashi ga yuku yo
ああ 心が笑いたがっている啊啊 内心渴望着欢笑aa kokoro ga waraitagatte iru
ひと知れず无人知晓hito shirezu
されど誇らかに咲け却仍要骄傲地绽放saredo hokoraka ni sake
ああ わたしは 冬の花啊啊 我就是 冬之花aa watashi wa fuyu no hana
胸には涙 顔には笑顔で泪藏心底 脸上却带着笑容mune ni wa namida kao ni wa egao de
今日もわたしは出かける今天 我依然要出发kyou mo watashi wa dekakeru
わたしという名の物語は最終章以我为名的故事 已是最终章watashi to iu na no monogatari wa saishuushou
悲しくって 泣いてるわけじゃない哭泣 并非因为悲伤kanashikutte naiteru wake janai
生きてるから涙が出るの只是因为活着 才会流泪ikiteru kara namida ga deru no
こごえる季節に鮮やかに咲くよ在这严寒季节 绚烂绽放吧kogoeru kisetsu ni azayaka ni saku yo
ああ わたしが負けるわけがない啊啊 我绝不会认输aa watashi ga makeru wake ga nai
泣かないで わたしの恋心别哭泣啊 我的爱恋之心nakanaide watashi no koigokoro
涙はお前には似合わない眼泪 与你并不相配namida wa omae ni wa niawanai
ゆけ ただゆけ走吧 只管向前走yuke tada yuke
いっそわたしがゆくよ索性 我便独自前行isso watashi ga yuku yo
ああ 心が笑いたがっている啊啊 内心渴望着欢笑aa kokoro ga waraitagatte iru
ひと知れず无人知晓hito shirezu
されど誇らかに咲け却仍要骄傲地绽放saredo hokoraka ni sake
ああ わたしは 冬の花啊啊 我就是 冬之花aa watashi wa fuyu no hana
胸には涙 顔には笑顔で泪藏心底 脸上却带着笑容mune ni wa namida kao ni wa egao de
今日もわたしは出かける今天 我依然要出发kyou mo watashi wa dekakeru".as_ptr();
        syscall(100, str as u64, 0, 0, 0, 0, 0);
    }
    loop {}
}

#[panic_handler]
pub fn panic_handler(_info: &PanicInfo) -> ! {
    loop {}
}
